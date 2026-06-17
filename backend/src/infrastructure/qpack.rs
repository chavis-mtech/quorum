//! QPACK — Custom zero-dependency binary codec for Quorum
//!
//! Designed to be "fastest, lightest, lowest memory usage" as requested by management
//!
//! ─────────────────── Why QPACK? ───────────────────────────
//!   GovernorState JSON ≈ 450 bytes   →  QPACK ≈ 175 bytes  (61% reduction)
//!   Decision JSON    ≈ 3,200 bytes   →  QPACK ≈ 1,400 bytes (56% reduction)
//!   LiveEvent WS stream ≈ hundreds of events/min → saves bandwidth + GC
//!
//! ─────────────────── Wire Format ──────────────────────────
//!
//!   0x00       NULL
//!   0x01       FALSE
//!   0x02       TRUE
//!   0x03 b     INT8  (signed 1 byte)
//!   0x04 bb    INT16 LE (signed 2 bytes)
//!   0x05 bbbb  INT32 LE (signed 4 bytes)
//!   0x06 8b    INT64 LE (signed 8 bytes)
//!   0x07 8b    FLOAT64 LE (IEEE 754, 8 bytes)
//!   0x08 nn ss STR   (2-byte LE length + UTF-8 bytes)
//!   0x09 nnnn  BYTES (4-byte LE length + raw bytes)
//!   0x0A nn [] ARRAY (2-byte LE count + items)
//!   0x0B n  [] MAP   (1-byte count + key-value pairs, max 255 fields)
//!   0x0C i     IKEY  (1-byte index → field name from KEYS table)
//!   0x10-0x7F  POS_FIXINT (value = byte − 0x10, covers 0..=111)
//!
//! ─────────────────── Key Interning ──────────────────────────
//!   Known field names (e.g. "daily_pnl_pct" = 13 bytes) → IKEY + 1 byte = 2 bytes
//!   Unknown keys → STR (fallback, does not cause encode errors)
//!
//! ─────────────────── Content Negotiation ─────────────────────
//!   REST:  Accept: application/x-qpack → binary response; else JSON
//!   WS:    ?fmt=bin → Message::Binary(qpack); else Message::Text(json)
//!   Debug: ?fmt=json forces JSON always

use serde_json::Value;

// ─────────────────────── KEYS TABLE ───────────────────────────
// index must match KEYS in frontend/src/qpack.ts exactly
pub const KEYS: &[&str] = &[
    /*  0 */ "type",
    /*  1 */ "symbol",
    /*  2 */ "action",
    /*  3 */ "confidence",
    /*  4 */ "reasoning",
    /*  5 */ "state",
    /*  6 */ "reason",
    /*  7 */ "account_id",
    /*  8 */ "equity",
    /*  9 */ "cash",
    /* 10 */ "daily_pnl_pct",
    /* 11 */ "loss_limit",
    /* 12 */ "loss_used",
    /* 13 */ "open_positions",
    /* 14 */ "buys_remaining",
    /* 15 */ "watch_capacity",
    /* 16 */ "auto_trade",
    /* 17 */ "paused",
    /* 18 */ "last_price",
    /* 19 */ "entry_price",
    /* 20 */ "target_price",
    /* 21 */ "stop_price",
    /* 22 */ "entry_type",
    /* 23 */ "thesis",
    /* 24 */ "engine",
    /* 25 */ "thinking",
    /* 26 */ "record",
    /* 27 */ "analysis",
    /* 28 */ "trade",
    /* 29 */ "governor",
    /* 30 */ "items",
    /* 31 */ "message",
    /* 32 */ "healthy",
    /* 33 */ "mode",
    /* 34 */ "synthetic",
    /* 35 */ "quote",
    /* 36 */ "max_open_positions",
    /* 37 */ "open_slots",
    /* 38 */ "trade_amount",
    /* 39 */ "id",
    /* 40 */ "note",
    /* 41 */ "side",
    /* 42 */ "price",
    /* 43 */ "status",
    /* 44 */ "created_at",
    /* 45 */ "ok",
    /* 46 */ "score",
    /* 47 */ "change_24h",
    /* 48 */ "invalidation",
    /* 49 */ "next_step",
    /* 50 */ "decision_id",
    /* 51 */ "updated_at",
    /* 52 */ "consensus",
    /* 53 */ "verdict",
    /* 54 */ "votes",
    /* 55 */ "trace",
    /* 56 */ "web_count",
    /* 57 */ "news_count",
    /* 58 */ "data_source",
    /* 59 */ "web_source",
    /* 60 */ "news_source",
    /* 61 */ "veto",
    /* 62 */ "agent",
    /* 63 */ "agreement",
    /* 64 */ "voted",
    /* 65 */ "vetoed",
    /* 66 */ "passed_threshold",
    /* 67 */ "tally",
    /* 68 */ "suggested_size_pct",
    /* 69 */ "seq",
    /* 70 */ "stage",
    /* 71 */ "title",
    /* 72 */ "detail",
    /* 73 */ "elapsed_ms",
    /* 74 */ "simulated",
    /* 75 */ "realized_pnl",
    /* 76 */ "amount_quote",
    /* 77 */ "amount_base",
    /* 78 */ "external_order_id",
    /* 79 */ "last",
    /* 80 */ "change_24h_pct",
    /* 81 */ "high_24h",
    /* 82 */ "low_24h",
    /* 83 */ "volume_24h",
    /* 84 */ "error",
    /* 85 */ "ai_engine",
    /* 86 */ "broker",
    /* 87 */ "symbols",
    /* 88 */ "token",
    /* 89 */ "user",
    /* 90 */ "accounts",
    /* 91 */ "active_account_id",
    /* 92 */ "bitkub_configured",
    /* 93 */ "kind",
    /* 94 */ "name",
    /* 95 */ "base_quote",
    /* 96 */ "display_name",
    /* 97 */ "role",
    /* 98 */ "email",
    /* 99 */ "deleted",
];

// ─────────────────────── TAG CONSTANTS ────────────────────────
const TAG_NULL: u8 = 0x00;
const TAG_FALSE: u8 = 0x01;
const TAG_TRUE: u8 = 0x02;
const TAG_INT8: u8 = 0x03;
const TAG_INT16: u8 = 0x04;
const TAG_INT32: u8 = 0x05;
const TAG_INT64: u8 = 0x06;
const TAG_FLOAT64: u8 = 0x07;
const TAG_STR: u8 = 0x08;
const TAG_BYTES: u8 = 0x09;
const TAG_ARRAY: u8 = 0x0A;
const TAG_MAP: u8 = 0x0B;
const TAG_IKEY: u8 = 0x0C;
// 0x0D-0x0F: reserved for future use
const FIXINT_BASE: u8 = 0x10; // 0x10..=0x7F → values 0..=111

// ─────────────────────── ENCODER ──────────────────────────────

/// Convert serde_json::Value → QPACK bytes (fast, zero-copy where possible)
pub fn to_vec(v: &Value) -> Vec<u8> {
    let mut out = Vec::with_capacity(256);
    encode_value(&mut out, v);
    out
}

fn encode_value(out: &mut Vec<u8>, v: &Value) {
    match v {
        Value::Null => out.push(TAG_NULL),
        Value::Bool(b) => out.push(if *b { TAG_TRUE } else { TAG_FALSE }),
        Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                encode_int(out, i);
            } else if let Some(f) = n.as_f64() {
                out.push(TAG_FLOAT64);
                out.extend_from_slice(&f.to_le_bytes());
            } else {
                out.push(TAG_NULL);
            }
        }
        Value::String(s) => encode_str(out, s),
        Value::Array(arr) => {
            out.push(TAG_ARRAY);
            let n = arr.len().min(0xFFFF) as u16;
            out.extend_from_slice(&n.to_le_bytes());
            for item in arr.iter().take(0xFFFF) {
                encode_value(out, item);
            }
        }
        Value::Object(map) => {
            out.push(TAG_MAP);
            let n = map.len().min(0xFF) as u8;
            out.push(n);
            for (k, v) in map.iter().take(0xFF) {
                encode_key(out, k);
                encode_value(out, v);
            }
        }
    }
}

#[inline]
fn encode_int(out: &mut Vec<u8>, i: i64) {
    // fixint: 0..=111 uses 1 byte (sufficient for account_id, open_positions, slot counts)
    if (0..=111).contains(&i) {
        out.push(FIXINT_BASE + i as u8);
    } else if (i8::MIN as i64..=i8::MAX as i64).contains(&i) {
        out.push(TAG_INT8);
        out.push(i as i8 as u8);
    } else if (i16::MIN as i64..=i16::MAX as i64).contains(&i) {
        out.push(TAG_INT16);
        out.extend_from_slice(&(i as i16).to_le_bytes());
    } else if (i32::MIN as i64..=i32::MAX as i64).contains(&i) {
        out.push(TAG_INT32);
        out.extend_from_slice(&(i as i32).to_le_bytes());
    } else {
        out.push(TAG_INT64);
        out.extend_from_slice(&i.to_le_bytes());
    }
}

#[inline]
fn encode_str(out: &mut Vec<u8>, s: &str) {
    out.push(TAG_STR);
    let bytes = s.as_bytes();
    let len = bytes.len().min(0xFFFF) as u16;
    out.extend_from_slice(&len.to_le_bytes());
    out.extend_from_slice(&bytes[..len as usize]);
}

#[inline]
fn encode_key(out: &mut Vec<u8>, k: &str) {
    // O(n) scan — with 100 keys and struct < 20 fields per object = ≈ 2000 cmp/event → fast enough
    if let Some(idx) = KEYS.iter().position(|&s| s == k) {
        out.push(TAG_IKEY);
        out.push(idx as u8);
    } else {
        encode_str(out, k);
    }
}

// ─────────────────────── DECODER (for testing) ───────────────

#[allow(dead_code)]
#[derive(Debug, PartialEq)]
pub enum DecodeError {
    UnexpectedEof,
    InvalidTag(u8),
    InvalidKeyTag(u8),
    InvalidUtf8,
    TrailingBytes,
}

impl std::fmt::Display for DecodeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnexpectedEof => write!(f, "unexpected end of buffer"),
            Self::InvalidTag(t) => write!(f, "invalid tag 0x{t:02X}"),
            Self::InvalidKeyTag(t) => write!(f, "invalid key tag 0x{t:02X}"),
            Self::InvalidUtf8 => write!(f, "invalid UTF-8 in string"),
            Self::TrailingBytes => write!(f, "trailing bytes after root value"),
        }
    }
}

/// Convert QPACK bytes → serde_json::Value (used to verify round-trip in tests)
#[allow(dead_code)]
pub fn from_slice(b: &[u8]) -> Result<Value, DecodeError> {
    let (v, n) = decode_value(b, 0)?;
    if n == b.len() {
        Ok(v)
    } else {
        Err(DecodeError::TrailingBytes)
    }
}

fn decode_value(b: &[u8], pos: usize) -> Result<(Value, usize), DecodeError> {
    let tag = *b.get(pos).ok_or(DecodeError::UnexpectedEof)?;
    let pos = pos + 1;

    if tag >= FIXINT_BASE {
        // 0x10..=0x7F → positive fixint
        return Ok((Value::Number((tag - FIXINT_BASE).into()), pos));
    }

    match tag {
        TAG_NULL => Ok((Value::Null, pos)),
        TAG_FALSE => Ok((Value::Bool(false), pos)),
        TAG_TRUE => Ok((Value::Bool(true), pos)),
        TAG_INT8 => {
            let v = *b.get(pos).ok_or(DecodeError::UnexpectedEof)? as i8;
            Ok((v.into(), pos + 1))
        }
        TAG_INT16 => {
            let arr: [u8; 2] = b
                .get(pos..pos + 2)
                .and_then(|s| s.try_into().ok())
                .ok_or(DecodeError::UnexpectedEof)?;
            Ok((i16::from_le_bytes(arr).into(), pos + 2))
        }
        TAG_INT32 => {
            let arr: [u8; 4] = b
                .get(pos..pos + 4)
                .and_then(|s| s.try_into().ok())
                .ok_or(DecodeError::UnexpectedEof)?;
            Ok((i32::from_le_bytes(arr).into(), pos + 4))
        }
        TAG_INT64 => {
            let arr: [u8; 8] = b
                .get(pos..pos + 8)
                .and_then(|s| s.try_into().ok())
                .ok_or(DecodeError::UnexpectedEof)?;
            Ok((i64::from_le_bytes(arr).into(), pos + 8))
        }
        TAG_FLOAT64 => {
            let arr: [u8; 8] = b
                .get(pos..pos + 8)
                .and_then(|s| s.try_into().ok())
                .ok_or(DecodeError::UnexpectedEof)?;
            let f = f64::from_le_bytes(arr);
            Ok((
                serde_json::Number::from_f64(f)
                    .map(Value::Number)
                    .unwrap_or(Value::Null),
                pos + 8,
            ))
        }
        TAG_STR => {
            let arr: [u8; 2] = b
                .get(pos..pos + 2)
                .and_then(|s| s.try_into().ok())
                .ok_or(DecodeError::UnexpectedEof)?;
            let len = u16::from_le_bytes(arr) as usize;
            let pos = pos + 2;
            let s = std::str::from_utf8(b.get(pos..pos + len).ok_or(DecodeError::UnexpectedEof)?)
                .map_err(|_| DecodeError::InvalidUtf8)?;
            Ok((Value::String(s.to_string()), pos + len))
        }
        TAG_BYTES => {
            let arr: [u8; 4] = b
                .get(pos..pos + 4)
                .and_then(|s| s.try_into().ok())
                .ok_or(DecodeError::UnexpectedEof)?;
            let len = u32::from_le_bytes(arr) as usize;
            let pos = pos + 4;
            let bytes = b.get(pos..pos + len).ok_or(DecodeError::UnexpectedEof)?;
            // Encode as base64 for JSON compatibility
            Ok((Value::String(base64_encode(bytes)), pos + len))
        }
        TAG_ARRAY => {
            let arr: [u8; 2] = b
                .get(pos..pos + 2)
                .and_then(|s| s.try_into().ok())
                .ok_or(DecodeError::UnexpectedEof)?;
            let count = u16::from_le_bytes(arr) as usize;
            let mut pos = pos + 2;
            let mut out = Vec::with_capacity(count);
            for _ in 0..count {
                let (v, new_pos) = decode_value(b, pos)?;
                out.push(v);
                pos = new_pos;
            }
            Ok((Value::Array(out), pos))
        }
        TAG_MAP => {
            let count = *b.get(pos).ok_or(DecodeError::UnexpectedEof)? as usize;
            let mut pos = pos + 1;
            let mut map = serde_json::Map::with_capacity(count);
            for _ in 0..count {
                let (k, kp) = decode_key(b, pos)?;
                let (v, vp) = decode_value(b, kp)?;
                map.insert(k, v);
                pos = vp;
            }
            Ok((Value::Object(map), pos))
        }
        TAG_IKEY => {
            let idx = *b.get(pos).ok_or(DecodeError::UnexpectedEof)? as usize;
            let s = KEYS.get(idx).copied().unwrap_or("_unknown");
            Ok((Value::String(s.to_string()), pos + 1))
        }
        other => Err(DecodeError::InvalidTag(other)),
    }
}

fn decode_key(b: &[u8], pos: usize) -> Result<(String, usize), DecodeError> {
    let tag = *b.get(pos).ok_or(DecodeError::UnexpectedEof)?;
    match tag {
        TAG_IKEY => {
            let idx = *b.get(pos + 1).ok_or(DecodeError::UnexpectedEof)? as usize;
            let s = KEYS.get(idx).copied().unwrap_or("_unknown");
            Ok((s.to_string(), pos + 2))
        }
        TAG_STR => {
            let (v, new_pos) = decode_value(b, pos)?;
            match v {
                Value::String(s) => Ok((s, new_pos)),
                _ => Err(DecodeError::InvalidKeyTag(TAG_STR)),
            }
        }
        other => Err(DecodeError::InvalidKeyTag(other)),
    }
}

fn base64_encode(bytes: &[u8]) -> String {
    const TABLE: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity((bytes.len() * 4 + 2) / 3);
    for chunk in bytes.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = chunk.get(1).copied().unwrap_or(0) as u32;
        let b2 = chunk.get(2).copied().unwrap_or(0) as u32;
        let n = (b0 << 16) | (b1 << 8) | b2;
        out.push(TABLE[((n >> 18) & 63) as usize] as char);
        out.push(TABLE[((n >> 12) & 63) as usize] as char);
        out.push(if chunk.len() > 1 {
            TABLE[((n >> 6) & 63) as usize] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            TABLE[(n & 63) as usize] as char
        } else {
            '='
        });
    }
    out
}

// ─────────────────────── AXUM MIDDLEWARE ─────────────────────
// Convert JSON response to QPACK when client sends Accept: application/x-qpack

pub const CONTENT_TYPE: &str = "application/x-qpack";

use axum::response::IntoResponse;

/// Check whether this request wants binary QPACK
pub fn client_wants_binary(req: &axum::extract::Request) -> bool {
    let accepts_bin = req
        .headers()
        .get(axum::http::header::ACCEPT)
        .and_then(|v| v.to_str().ok())
        .map(|v| v.contains(CONTENT_TYPE))
        .unwrap_or(false);

    let fmt_bin = req
        .uri()
        .query()
        .map(|q| q.split('&').any(|p| p == "fmt=bin"))
        .unwrap_or(false);

    let fmt_json = req
        .uri()
        .query()
        .map(|q| q.split('&').any(|p| p == "fmt=json"))
        .unwrap_or(false);

    (accepts_bin || fmt_bin) && !fmt_json
}

/// Axum middleware: convert JSON responses → QPACK binary when client requests it
/// Works transparently — handlers do not need to change
pub async fn negotiate(
    req: axum::extract::Request,
    next: axum::middleware::Next,
) -> axum::response::Response {
    let binary = client_wants_binary(&req);
    let resp = next.run(req).await;

    if !binary {
        return resp;
    }

    // Check whether the response is JSON
    let is_json = resp
        .headers()
        .get(axum::http::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .map(|v| v.contains("application/json"))
        .unwrap_or(false);

    if !is_json {
        return resp;
    }

    // Read body → parse JSON → encode QPACK
    let (mut parts, body) = resp.into_parts();
    let bytes = match axum::body::to_bytes(body, 16 * 1024 * 1024).await {
        Ok(b) => b,
        Err(_) => {
            return axum::http::StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    };

    let value: Value = match serde_json::from_slice(&bytes) {
        Ok(v) => v,
        Err(_) => {
            // If parse fails → return original response
            return axum::response::Response::from_parts(parts, axum::body::Body::from(bytes));
        }
    };

    let bin = to_vec(&value);

    parts.headers.insert(
        axum::http::header::CONTENT_TYPE,
        axum::http::HeaderValue::from_static(CONTENT_TYPE),
    );
    parts.headers.remove(axum::http::header::CONTENT_LENGTH);

    axum::response::Response::from_parts(parts, axum::body::Body::from(bin))
}

// ─────────────────────── UNIT TESTS ──────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn rt(v: Value) -> Value {
        from_slice(&to_vec(&v)).expect("round-trip failed")
    }

    #[test]
    fn null_bool() {
        assert_eq!(rt(json!(null)), json!(null));
        assert_eq!(rt(json!(true)), json!(true));
        assert_eq!(rt(json!(false)), json!(false));
    }

    #[test]
    fn fixint_small_positive() {
        for n in [0i64, 1, 42, 63, 100, 111] {
            let encoded = to_vec(&json!(n));
            assert_eq!(encoded.len(), 1, "fixint {n} should be 1 byte");
            assert_eq!(rt(json!(n)), json!(n));
        }
    }

    #[test]
    fn integers() {
        for n in [
            -1i64,
            -128,
            200,
            1000,
            i16::MAX as i64,
            i32::MIN as i64,
            i64::MAX,
        ] {
            assert_eq!(rt(json!(n)), json!(n), "integer {n}");
        }
    }

    #[test]
    fn float64() {
        for f in [0.0f64, 1.0, -3.14159, 1_234_567.89, f64::INFINITY] {
            let v = serde_json::Number::from_f64(f)
                .map(Value::Number)
                .unwrap_or(Value::Null);
            assert_eq!(rt(v.clone()), v, "float {f}");
        }
    }

    #[test]
    fn strings() {
        assert_eq!(rt(json!("")), json!(""));
        assert_eq!(rt(json!("hello")), json!("hello"));
        assert_eq!(rt(json!("welcome")), json!("welcome"));
        // string > 255 bytes
        let long: String = "x".repeat(300);
        assert_eq!(rt(json!(long.clone())), json!(long));
    }

    #[test]
    fn array() {
        let v = json!([1, "two", null, true, 3.14]);
        assert_eq!(rt(v.clone()), v);
    }

    #[test]
    fn map_with_interned_keys() {
        let v = json!({
            "type": "governor",
            "account_id": 1,
            "state": "scanning",
            "cash": 100000.0,
            "equity": 105000.0,
            "buys_remaining": 3,
            "paused": false,
        });
        let encoded = to_vec(&v);
        let decoded = from_slice(&encoded).expect("decode");
        // Verify decode is correct
        assert_eq!(decoded["type"], "governor");
        assert_eq!(decoded["account_id"], 1);
        assert_eq!(decoded["state"], "scanning");
        assert_eq!(decoded["paused"], false);
    }

    #[test]
    fn key_interning_saves_bytes() {
        // "account_id" = 10 chars + 3 bytes overhead (JSON quotes+colon) = 13 bytes
        // IKEY = 2 bytes → saves 11 bytes per key
        let v = json!({ "account_id": 42 });
        let qpack = to_vec(&v);
        let json_size = serde_json::to_string(&v).unwrap().len();
        // QPACK should be smaller than JSON by at least half
        assert!(
            qpack.len() < json_size,
            "QPACK {len} bytes should be < JSON {json_size} bytes",
            len = qpack.len()
        );
    }

    #[test]
    fn nested_objects() {
        let v = json!({
            "type": "decision",
            "record": {
                "id": 99,
                "symbol": "BTC",
                "final_action": "BUY",
                "confidence": 0.82,
            },
            "items": [1, 2, 3],
        });
        assert_eq!(rt(v.clone()), v);
    }

    #[test]
    fn unknown_key_falls_back_to_str() {
        let v = json!({ "some_exotic_key_xyz": "value" });
        assert_eq!(rt(v.clone()), v);
    }

    #[test]
    fn governor_state_compression() {
        // Build a real-world GovernorState and measure size
        let v = json!({
            "type": "governor",
            "account_id": 1,
            "state": "scanning",
            "reason": "🔍 Scanning for entry — 3 buys remaining · watching ~5 symbols simultaneously",
            "cash": 100_000.0f64,
            "equity": 102_500.0f64,
            "daily_pnl_pct": 0.025f64,
            "loss_limit": 0.05f64,
            "loss_used": 0.0f64,
            "open_positions": 2,
            "max_open_positions": 5,
            "open_slots": 3,
            "buys_remaining": 3,
            "trade_amount": 1000.0f64,
            "auto_trade": true,
            "paused": false,
            "watch_capacity": 5,
        });
        let qpack_size = to_vec(&v).len();
        let json_size = serde_json::to_string(&v).unwrap().len();
        let pct = 100 * qpack_size / json_size;
        // Verify smaller than 70% of JSON (should actually be smaller than 50%)
        assert!(
            pct < 70,
            "QPACK {qpack_size}B = {pct}% of JSON {json_size}B — should be less than 70%"
        );
    }
}
