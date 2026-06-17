/**
 * QPACK decoder — decode binary → JavaScript value
 *
 * Must match the format in backend/src/infrastructure/qpack.rs exactly
 * Zero dependencies, ~150 lines, operates on DataView (TypedArray API)
 *
 * Wire format:
 *   0x00       NULL
 *   0x01       FALSE
 *   0x02       TRUE
 *   0x03 b     INT8  (signed)
 *   0x04 bb    INT16 LE
 *   0x05 bbbb  INT32 LE
 *   0x06 8b    INT64 LE (as JS number, safe integer range)
 *   0x07 8b    FLOAT64 LE
 *   0x08 nn ss STR   (2-byte LE len + UTF-8)
 *   0x09 nnnn  BYTES (4-byte LE len + bytes → base64 string)
 *   0x0A nn [] ARRAY (2-byte LE count + items)
 *   0x0B n  [] MAP   (1-byte count + key-value pairs)
 *   0x0C i     IKEY  (1-byte index into KEYS table)
 *   0x10-0x7F  POS_FIXINT (value = byte − 0x10, range 0..=111)
 */

// ─────────────────── Keys table ──────────────────────────────
// Must match KEYS in backend/src/infrastructure/qpack.rs exactly (index order matters)
export const KEYS: readonly string[] = [
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

// ─────────────────── Tag constants ────────────────────────────
const TAG_NULL    = 0x00;
const TAG_FALSE   = 0x01;
const TAG_TRUE    = 0x02;
const TAG_INT8    = 0x03;
const TAG_INT16   = 0x04;
const TAG_INT32   = 0x05;
const TAG_INT64   = 0x06;
const TAG_FLOAT64 = 0x07;
const TAG_STR     = 0x08;
const TAG_BYTES   = 0x09;
const TAG_ARRAY   = 0x0A;
const TAG_MAP     = 0x0B;
const TAG_IKEY    = 0x0C;
const FIXINT_BASE = 0x10; // 0x10..0x7F → values 0..111

// ─────────────────── Shared TextDecoder ───────────────────────
// Created once — no need to allocate a new instance for every string
const _td = new TextDecoder("utf-8");

// ─────────────────── Core decode functions ────────────────────

interface State {
  view: DataView;
  pos: number;
}

function readU8(s: State): number {
  return s.view.getUint8(s.pos++);
}

function readStr(s: State): string {
  const len = s.view.getUint16(s.pos, true);
  s.pos += 2;
  const bytes = new Uint8Array(s.view.buffer, s.view.byteOffset + s.pos, len);
  s.pos += len;
  return _td.decode(bytes);
}

function decodeKey(s: State): string {
  const tag = readU8(s);
  if (tag === TAG_IKEY) {
    const idx = readU8(s);
    return KEYS[idx] ?? `_k${idx}`;
  }
  if (tag === TAG_STR) {
    return readStr(s);
  }
  throw new Error(`Invalid QPACK key tag: 0x${tag.toString(16)}`);
}

function decodeValue(s: State): unknown {
  const tag = readU8(s);

  // Positive fixint fast path (most common: small IDs, counts)
  if (tag >= FIXINT_BASE) return tag - FIXINT_BASE;

  switch (tag) {
    case TAG_NULL:    return null;
    case TAG_FALSE:   return false;
    case TAG_TRUE:    return true;

    case TAG_INT8: {
      const v = s.view.getInt8(s.pos); s.pos += 1; return v;
    }
    case TAG_INT16: {
      const v = s.view.getInt16(s.pos, true); s.pos += 2; return v;
    }
    case TAG_INT32: {
      const v = s.view.getInt32(s.pos, true); s.pos += 4; return v;
    }
    case TAG_INT64: {
      // Use BigInt for correctness, then convert back to number
      // (i64 values in our domain always fit within JS safe integer range)
      const lo = BigInt(s.view.getUint32(s.pos, true));
      const hi = BigInt(s.view.getInt32(s.pos + 4, true));
      s.pos += 8;
      return Number(hi * 0x100000000n + lo);
    }
    case TAG_FLOAT64: {
      const v = s.view.getFloat64(s.pos, true); s.pos += 8; return v;
    }
    case TAG_STR: {
      return readStr(s);
    }
    case TAG_BYTES: {
      const len = s.view.getUint32(s.pos, true); s.pos += 4;
      const bytes = new Uint8Array(s.view.buffer, s.view.byteOffset + s.pos, len);
      s.pos += len;
      // Convert to base64 string (matches Rust decoder)
      const chars = Array.from(bytes, (b) => String.fromCharCode(b));
      return btoa(chars.join(""));
    }
    case TAG_ARRAY: {
      const count = s.view.getUint16(s.pos, true); s.pos += 2;
      const arr: unknown[] = new Array(count);
      for (let i = 0; i < count; i++) arr[i] = decodeValue(s);
      return arr;
    }
    case TAG_MAP: {
      const count = readU8(s);
      const obj: Record<string, unknown> = Object.create(null);
      for (let i = 0; i < count; i++) {
        const key = decodeKey(s);
        obj[key] = decodeValue(s);
      }
      return obj;
    }
    case TAG_IKEY: {
      // IKEY as standalone value (should not occur, but kept as fallback)
      const idx = readU8(s);
      return KEYS[idx] ?? `_k${idx}`;
    }
    default:
      throw new Error(`Unknown QPACK tag: 0x${tag.toString(16)} at pos ${s.pos - 1}`);
  }
}

// ─────────────────── Public API ───────────────────────────────

/** Decode QPACK binary (ArrayBuffer or Uint8Array) → JavaScript value */
export function qpackDecode(buf: ArrayBuffer | Uint8Array): unknown {
  const ab = buf instanceof Uint8Array ? buf.buffer : buf;
  const offset = buf instanceof Uint8Array ? buf.byteOffset : 0;
  const len = buf instanceof Uint8Array ? buf.byteLength : buf.byteLength;
  const s: State = { view: new DataView(ab, offset, len), pos: 0 };
  return decodeValue(s);
}

/** Content-type indicating QPACK binary */
export const QPACK_CONTENT_TYPE = "application/x-qpack";

/** Accept header to request a binary response */
export const QPACK_ACCEPT = "application/x-qpack";
