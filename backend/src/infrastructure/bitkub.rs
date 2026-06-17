//! Broker adapter: Bitkub (real private API) + Paper
//!
//! Bitkub API v3 uses HMAC-SHA256 signing:
//!   payload = timestamp + METHOD + path + body
//!   sign    = hex(hmac_sha256(api_secret, payload))
//!   headers: X-BTK-APIKEY, X-BTK-TIMESTAMP, X-BTK-SIGN
//!
//! Supports: fetching balances for all coins, placing market orders (place-bid/place-ask)

use async_trait::async_trait;
use chrono::{TimeZone, Utc};
use hmac::{Hmac, Mac};
use serde_json::{json, Value};
use sha2::Sha256;
use tokio::sync::RwLock;

use crate::domain::models::{
    Action, Balance, BrokerCredentials, BrokerOrder, OpenOrder, OrderRequest, OrderResult,
};
use crate::domain::ports::{Broker, DomainError, DomainResult};

const BASE: &str = "https://api.bitkub.com";
type HmacSha256 = Hmac<Sha256>;

/// Translate Bitkub v3 error codes into human-readable help messages
fn bitkub_error_msg(code: i64) -> String {
    let m = match code {
        6 => "invalid signature",
        3 => "invalid API key",
        5 => "IP not allowed — add your IP to the API key settings",
        11 => "invalid symbol",
        15 => "amount too low (below Bitkub minimum)",
        16 => "cannot read balance — API key must have Wallet/Balance permission enabled",
        17 => "empty wallet",
        18 => "insufficient balance",
        19 => "failed to place order in order book — usually caused by symbol/amount/market state not matching Bitkub specs",
        25 => "KYC level 1 verification required",
        52 => "API key lacks permission for this action — enable Trade/Wallet permissions in Bitkub API key settings",
        56 => "account temporarily suspended from buying",
        57 => "account temporarily suspended from selling",
        61 => "this coin is a Bitkub 'broker' listing — it cannot be traded through the standard order API (only 'exchange'-listed coins are tradable here)",
        90 => "Bitkub server error",
        _ => "see Bitkub API documentation for error code details",
    };
    m.to_string()
}

pub async fn public_last_price(symbol: &str, quote: &str) -> DomainResult<f64> {
    let pair = format!("{}_{}", symbol.to_uppercase(), quote.to_uppercase());
    let legacy_pair = format!("{}_{}", quote.to_uppercase(), symbol.to_uppercase());
    let raw: Value = reqwest::get(format!("{BASE}/api/v3/market/ticker"))
        .await
        .map_err(|e| DomainError::Broker(e.to_string()))?
        .json()
        .await
        .map_err(|e| DomainError::Broker(e.to_string()))?;
    if let Some(arr) = raw.as_array() {
        return arr
            .iter()
            .find(|v| {
                v.get("symbol")
                    .and_then(|s| s.as_str())
                    .map(|s| s.eq_ignore_ascii_case(&pair))
                    .unwrap_or(false)
            })
            .and_then(|v| v.get("last"))
            .and_then(json_f64)
            .ok_or_else(|| DomainError::Broker(format!("price not found for {pair}")));
    }
    // Fallback for legacy endpoint in case Bitkub changes response format between deploys
    raw.get(&legacy_pair)
        .and_then(|v| v.get("last"))
        .and_then(json_f64)
        .ok_or_else(|| DomainError::Broker(format!("price not found for {pair}")))
}

#[derive(Debug, Clone)]
struct MarketSymbol {
    api_symbol: String,
    legacy_symbol: String,
    min_quote_size: f64,
    base_asset_scale: u32,
    freeze_buy: bool,
    freeze_sell: bool,
    status: String,
    /// "exchange" = traded on Bitkub's own order book (tradable via this API);
    /// "broker" = routed to a third-party broker → the order API rejects it (error 61)
    source: String,
}

#[derive(Debug)]
struct BitkubOrderError {
    code: i64,
    path: String,
    body: String,
}

impl BitkubOrderError {
    fn into_domain(self) -> DomainError {
        DomainError::Broker(format!(
            "Bitkub error {}: {} (request {} {})",
            self.code,
            bitkub_error_msg(self.code),
            self.path,
            self.body
        ))
    }
}

fn json_f64(v: &Value) -> Option<f64> {
    v.as_f64()
        .or_else(|| v.as_str().and_then(|s| s.parse().ok()))
}

fn positive_json_f64(v: &Value) -> Option<f64> {
    json_f64(v).filter(|n| n.is_finite() && *n > 0.0)
}

fn bitkub_number(value: f64, scale: u32) -> DomainResult<String> {
    if !value.is_finite() {
        return Err(DomainError::Broker("invalid order amount".into()));
    }
    let scale = scale.min(12) as usize;
    let mut out = format!("{value:.scale$}");
    while out.contains('.') && out.ends_with('0') {
        out.pop();
    }
    if out.ends_with('.') {
        out.pop();
    }
    if out == "-0" {
        out = "0".into();
    }
    Ok(out)
}

fn round_down(value: f64, scale: u32) -> f64 {
    let factor = 10_f64.powi(scale.min(12) as i32);
    (value * factor).floor() / factor
}

fn bitkub_order_body(sym: &str, amount: f64, amount_scale: u32) -> DomainResult<String> {
    let sym = serde_json::to_string(sym).map_err(|e| DomainError::Broker(e.to_string()))?;
    let amt = bitkub_number(amount, amount_scale)?;
    Ok(format!(
        r#"{{"sym":{sym},"amt":{amt},"rat":0,"typ":"market"}}"#
    ))
}

/// Percent-encode cursor value (base64 may contain + / =) for safe use in query string.
/// Must encode before both signing and sending so the payload matches the outgoing string.
fn encode_cursor(c: &str) -> String {
    let mut out = String::with_capacity(c.len() + 8);
    for b in c.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// Parse one entry from my-order-history into a BrokerOrder (None if data is incomplete or not buy/sell)
fn parse_history_order(it: &Value, symbol: &str, quote: &str) -> Option<BrokerOrder> {
    let side = Action::parse(it.get("side").and_then(|v| v.as_str()).unwrap_or(""));
    if !matches!(side, Action::Buy | Action::Sell) {
        return None;
    }
    let price = it.get("rate").and_then(json_f64).unwrap_or(0.0);
    let amount = it.get("amount").and_then(json_f64).unwrap_or(0.0);
    if price <= 0.0 || amount <= 0.0 {
        return None;
    }
    let (amount_base, amount_quote) = match side {
        Action::Buy => (amount / price, amount),
        Action::Sell => (amount, amount * price),
        Action::Hold => (0.0, 0.0),
    };
    let ts = it
        .get("order_closed_at")
        .or_else(|| it.get("ts"))
        .and_then(|v| v.as_i64())
        .unwrap_or_else(|| Utc::now().timestamp_millis());
    let created_at = Utc
        .timestamp_millis_opt(ts)
        .single()
        .unwrap_or_else(Utc::now);
    let order_id = it
        .get("order_id")
        .or_else(|| it.get("txn_id"))
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    if order_id.is_empty() {
        return None;
    }
    Some(BrokerOrder {
        order_id,
        symbol: symbol.to_uppercase(),
        quote: quote.to_uppercase(),
        side,
        amount_base,
        amount_quote,
        price,
        fee_quote: it.get("fee").and_then(json_f64).unwrap_or(0.0),
        created_at,
    })
}

// ============ Bitkub broker (real private API) ============

pub struct BitkubBroker {
    creds: RwLock<Option<BrokerCredentials>>,
    http: reqwest::Client,
}

impl BitkubBroker {
    pub fn new(creds: Option<BrokerCredentials>) -> Self {
        Self {
            creds: RwLock::new(creds),
            http: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(15))
                .build()
                .expect("http client"),
        }
    }

    pub async fn set_credentials(&self, creds: Option<BrokerCredentials>) {
        *self.creds.write().await = creds;
    }

    async fn credentials(&self, missing: &str) -> DomainResult<BrokerCredentials> {
        self.creds
            .read()
            .await
            .clone()
            .ok_or_else(|| DomainError::Broker(missing.into()))
    }

    async fn server_time(&self) -> DomainResult<String> {
        let txt = self
            .http
            .get(format!("{BASE}/api/v3/servertime"))
            .send()
            .await
            .map_err(|e| DomainError::Broker(e.to_string()))?
            .text()
            .await
            .map_err(|e| DomainError::Broker(e.to_string()))?;
        Ok(txt.trim().trim_matches('"').to_string())
    }

    fn sign(secret: &str, payload: &str) -> String {
        let mut mac = HmacSha256::new_from_slice(secret.as_bytes()).expect("hmac key");
        mac.update(payload.as_bytes());
        hex::encode(mac.finalize().into_bytes())
    }

    /// Send signed GET (Bitkub v4) — payload = ts + "GET" + path + query
    async fn signed_get_v4(&self, path: &str, query: &str) -> DomainResult<Value> {
        let creds = self.credentials("API key not configured").await?;
        let ts = self.server_time().await?;
        let payload = format!("{ts}GET{path}{query}");
        let sign = Self::sign(&creds.api_secret, &payload);
        let resp: Value = self
            .http
            .get(format!("{BASE}{path}{query}"))
            .header("Accept", "application/json")
            .header("Content-Type", "application/json")
            .header("X-BTK-APIKEY", creds.api_key)
            .header("X-BTK-TIMESTAMP", &ts)
            .header("X-BTK-SIGN", sign)
            .send()
            .await
            .map_err(|e| DomainError::Broker(e.to_string()))?
            .json()
            .await
            .map_err(|e| DomainError::Broker(e.to_string()))?;
        // v3-style error envelope {"error":N}
        if let Some(err) = resp.get("error").and_then(|v| v.as_i64()) {
            if err != 0 {
                return Err(DomainError::Broker(format!(
                    "Bitkub error {err}: {}",
                    bitkub_error_msg(err)
                )));
            }
        }
        // v4-style envelope {"code":"0"|"Axxxx", "message":..., "data":[...]}
        if let Some(code) = resp.get("code").and_then(|v| v.as_str()) {
            if code != "0" {
                let msg = resp.get("message").and_then(|v| v.as_str()).unwrap_or("");
                let hint = if msg.to_lowercase().contains("ip") {
                    " — add your machine's IP to the API key whitelist at Bitkub"
                } else {
                    ""
                };
                return Err(DomainError::Broker(format!("Bitkub {code}: {msg}{hint}")));
            }
        }
        Ok(resp.get("data").cloned().unwrap_or(json!([])))
    }

    async fn market_symbol(&self, symbol: &str, quote: &str) -> DomainResult<MarketSymbol> {
        let symbol = symbol.trim().to_uppercase();
        let quote = quote.trim().to_uppercase();
        let resp: Value = self
            .http
            .get(format!("{BASE}/api/v3/market/symbols"))
            .send()
            .await
            .map_err(|e| DomainError::Broker(e.to_string()))?
            .json()
            .await
            .map_err(|e| DomainError::Broker(e.to_string()))?;

        let err = resp.get("error").and_then(|v| v.as_i64()).unwrap_or(-1);
        if err != 0 {
            return Err(DomainError::Broker(format!(
                "Bitkub error {err}: {}",
                bitkub_error_msg(err)
            )));
        }

        let result = resp
            .get("result")
            .and_then(|v| v.as_array())
            .ok_or_else(|| DomainError::Broker("failed to read symbol list from Bitkub".into()))?;
        let wanted = format!("{symbol}_{quote}");
        let item = result.iter().find(|it| {
            let base_ok = it
                .get("base_asset")
                .and_then(|v| v.as_str())
                .map(|s| s.eq_ignore_ascii_case(&symbol))
                .unwrap_or(false);
            let quote_ok = it
                .get("quote_asset")
                .and_then(|v| v.as_str())
                .map(|s| s.eq_ignore_ascii_case(&quote))
                .unwrap_or(false);
            let symbol_ok = it
                .get("symbol")
                .and_then(|v| v.as_str())
                .map(|s| s.eq_ignore_ascii_case(&wanted))
                .unwrap_or(false);
            (base_ok && quote_ok) || symbol_ok
        });

        let item = item.ok_or_else(|| DomainError::Broker(format!("trading pair {wanted} not found")))?;
        let api_symbol = item
            .get("symbol")
            .and_then(|v| v.as_str())
            .unwrap_or(&wanted)
            .to_lowercase();
        let legacy_symbol = format!("{}_{}", quote.to_lowercase(), symbol.to_lowercase());
        let status = item
            .get("status")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        Ok(MarketSymbol {
            api_symbol,
            legacy_symbol,
            min_quote_size: item
                .get("min_quote_size")
                .and_then(json_f64)
                .unwrap_or(10.0),
            base_asset_scale: item
                .get("base_asset_scale")
                .and_then(|v| v.as_u64())
                .unwrap_or(8) as u32,
            freeze_buy: item
                .get("freeze_buy")
                .and_then(|v| v.as_bool())
                .unwrap_or(false),
            freeze_sell: item
                .get("freeze_sell")
                .and_then(|v| v.as_bool())
                .unwrap_or(false),
            status,
            // default to "exchange" when the field is absent so a missing field never
            // wrongly blocks a tradable coin (fail open — the explicit "broker" guard below
            // only triggers on a confirmed broker listing)
            source: item
                .get("source")
                .and_then(|v| v.as_str())
                .unwrap_or("exchange")
                .to_string(),
        })
    }

    /// Send signed POST to path (e.g. "/api/v3/market/place-bid") with a JSON body string
    async fn signed_order_post(
        &self,
        path: &str,
        body_str: &str,
    ) -> Result<Value, BitkubOrderError> {
        let creds = self
            .credentials("API key not configured (open modal to enter key first)")
            .await
            .map_err(|_| BitkubOrderError {
                code: -1,
                path: path.to_string(),
                body: body_str.to_string(),
            })?;
        let ts = self.server_time().await.map_err(|_| BitkubOrderError {
            code: -1,
            path: path.to_string(),
            body: body_str.to_string(),
        })?;
        let payload = format!("{ts}POST{path}{body_str}");
        let sign = Self::sign(&creds.api_secret, &payload);

        let resp: Value = self
            .http
            .post(format!("{BASE}{path}"))
            .header("Accept", "application/json")
            .header("Content-Type", "application/json")
            .header("X-BTK-APIKEY", creds.api_key)
            .header("X-BTK-TIMESTAMP", &ts)
            .header("X-BTK-SIGN", sign)
            .body(body_str.to_string())
            .send()
            .await
            .map_err(|_| BitkubOrderError {
                code: -1,
                path: path.to_string(),
                body: body_str.to_string(),
            })?
            .json()
            .await
            .map_err(|_| BitkubOrderError {
                code: -1,
                path: path.to_string(),
                body: body_str.to_string(),
            })?;

        // Bitkub v3: error == 0 means success
        let err = resp.get("error").and_then(|v| v.as_i64()).unwrap_or(-1);
        if err != 0 {
            tracing::warn!(
                path,
                body = %body_str,
                "Bitkub rejected order: error={err}"
            );
            return Err(BitkubOrderError {
                code: err,
                path: path.to_string(),
                body: body_str.to_string(),
            });
        }
        Ok(resp.get("result").cloned().unwrap_or(json!({})))
    }

    /// Send signed GET (Bitkub v3) returning the full envelope (with result + pagination) — payload = ts + "GET" + path + query
    async fn signed_get_v3_full(&self, path: &str, query: &str) -> DomainResult<Value> {
        let creds = self
            .credentials("API key not configured (open modal to enter key first)")
            .await?;
        let ts = self.server_time().await?;
        let payload = format!("{ts}GET{path}{query}");
        let sign = Self::sign(&creds.api_secret, &payload);
        let resp: Value = self
            .http
            .get(format!("{BASE}{path}{query}"))
            .header("Accept", "application/json")
            .header("Content-Type", "application/json")
            .header("X-BTK-APIKEY", creds.api_key)
            .header("X-BTK-TIMESTAMP", &ts)
            .header("X-BTK-SIGN", sign)
            .send()
            .await
            .map_err(|e| DomainError::Broker(e.to_string()))?
            .json()
            .await
            .map_err(|e| DomainError::Broker(e.to_string()))?;

        let err = resp.get("error").and_then(|v| v.as_i64()).unwrap_or(-1);
        if err != 0 {
            return Err(DomainError::Broker(format!(
                "Bitkub error {err}: {}",
                bitkub_error_msg(err)
            )));
        }
        Ok(resp)
    }

    async fn signed_get_v3(&self, path: &str, query: &str) -> DomainResult<Value> {
        Ok(self
            .signed_get_v3_full(path, query)
            .await?
            .get("result")
            .cloned()
            .unwrap_or(json!({})))
    }

    async fn order_info(&self, sym: &str, order_id: &str, action: Action) -> DomainResult<Value> {
        let side = match action {
            Action::Buy => "buy",
            Action::Sell => "sell",
            Action::Hold => return Ok(json!({})),
        };
        let query = format!("?sym={sym}&id={order_id}&sd={side}");
        self.signed_get_v3("/api/v3/market/order-info", &query)
            .await
    }

    async fn place_with_symbol_fallback(
        &self,
        path: &str,
        meta: &MarketSymbol,
        amount: f64,
        amount_scale: u32,
    ) -> DomainResult<(Value, String)> {
        let mut symbols = vec![meta.api_symbol.as_str()];
        if meta.legacy_symbol != meta.api_symbol {
            symbols.push(meta.legacy_symbol.as_str());
        }

        let mut last_err = None;
        for sym in symbols {
            let body = bitkub_order_body(sym, amount, amount_scale)?;
            match self.signed_order_post(path, &body).await {
                Ok(result) => return Ok((result, sym.to_string())),
                Err(e) if matches!(e.code, 11 | 19) && sym == meta.api_symbol => {
                    tracing::warn!(
                        path,
                        body = %body,
                        legacy_symbol = %meta.legacy_symbol,
                        "Bitkub rejected current symbol format; retrying legacy format once"
                    );
                    last_err = Some(e);
                }
                Err(e) => return Err(e.into_domain()),
            }
        }

        Err(last_err
            .map(BitkubOrderError::into_domain)
            .unwrap_or_else(|| DomainError::Broker("failed to place Bitkub order".into())))
    }
}

#[async_trait]
impl Broker for BitkubBroker {
    fn name(&self) -> &str {
        "bitkub"
    }
    fn is_simulated(&self) -> bool {
        false
    }
    async fn last_price(&self, symbol: &str, quote: &str) -> DomainResult<f64> {
        public_last_price(symbol, quote).await
    }

    async fn balance(&self, asset: &str) -> DomainResult<Balance> {
        let all = self.balances().await?;
        Ok(all
            .into_iter()
            .find(|b| b.asset.eq_ignore_ascii_case(asset))
            .unwrap_or(Balance {
                asset: asset.to_string(),
                available: 0.0,
            }))
    }

    async fn balances(&self) -> DomainResult<Vec<Balance>> {
        // v3 balances removed (2026-05-26) → use v4 GET /api/v4/wallet/balances
        let data = self.signed_get_v4("/api/v4/wallet/balances", "").await?;
        let mut out = Vec::new();
        if let Some(arr) = data.as_array() {
            for it in arr {
                let asset = it
                    .get("currency")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let avail = it
                    .get("available")
                    .and_then(|v| {
                        v.as_f64()
                            .or_else(|| v.as_str().and_then(|s| s.parse().ok()))
                    })
                    .unwrap_or(0.0);
                if avail > 0.0 {
                    out.push(Balance {
                        asset,
                        available: avail,
                    });
                }
            }
        }
        out.sort_by(|a, b| a.asset.cmp(&b.asset));
        Ok(out)
    }

    async fn order_history(
        &self,
        symbol: &str,
        quote: &str,
        limit: usize,
    ) -> DomainResult<Vec<BrokerOrder>> {
        // Walk all pages using keyset cursor until exhausted (or page cap to prevent runaway loops)
        // to accurately reconstruct average cost basis — not capped at first 50/100 entries like before
        let meta = self.market_symbol(symbol, quote).await?;
        let sym = meta.api_symbol.to_uppercase();
        let want = limit.max(1);
        const PAGE: usize = 100;
        const MAX_PAGES: usize = 60; // runaway guard: max ~6000 entries
        let mut out = Vec::new();
        let mut cursor: Option<String> = None;
        let mut pages = 0usize;
        loop {
            pages += 1;
            let mut query = format!("?sym={sym}&lmt={PAGE}&pagination_type=keyset");
            if let Some(c) = &cursor {
                query.push_str("&cursor=");
                query.push_str(&encode_cursor(c));
            }
            let resp = self
                .signed_get_v3_full("/api/v3/market/my-order-history", &query)
                .await?;
            let rows = resp.get("result").and_then(|v| v.as_array());
            let got = rows.map(|r| r.len()).unwrap_or(0);
            if let Some(rows) = rows {
                for it in rows {
                    if let Some(o) = parse_history_order(it, symbol, quote) {
                        out.push(o);
                    }
                }
            }
            let pag = resp.get("pagination");
            let has_next = pag
                .and_then(|p| p.get("has_next"))
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            let next = pag
                .and_then(|p| p.get("cursor"))
                .and_then(|v| v.as_str())
                .filter(|s| !s.is_empty())
                .map(|s| s.to_string());
            if out.len() >= want || got == 0 || !has_next || next.is_none() {
                break;
            }
            if pages >= MAX_PAGES {
                tracing::warn!(
                    symbol,
                    pages,
                    "my-order-history exceeded {MAX_PAGES} pages — history truncated, average cost basis may be inaccurate"
                );
                break;
            }
            cursor = next;
        }
        Ok(out)
    }

    async fn open_orders(&self, symbol: &str, quote: &str) -> DomainResult<Vec<OpenOrder>> {
        let meta = self.market_symbol(symbol, quote).await?;
        let sym = meta.api_symbol.to_uppercase();
        let query = format!("?sym={sym}");
        let data = self
            .signed_get_v3("/api/v3/market/my-open-orders", &query)
            .await?;
        let arr = match data.as_array() {
            Some(a) => a,
            None => return Ok(Vec::new()),
        };
        let mut out = Vec::new();
        for it in arr {
            let side = Action::parse(it.get("side").and_then(|v| v.as_str()).unwrap_or(""));
            if !matches!(side, Action::Buy | Action::Sell) {
                continue;
            }
            let order_id = it
                .get("id")
                .or_else(|| it.get("order_id"))
                .map(|v| match v {
                    Value::String(s) => s.clone(),
                    other => other.to_string(),
                })
                .unwrap_or_default();
            let price = it.get("rate").and_then(json_f64).unwrap_or(0.0);
            let amount = it.get("amount").and_then(json_f64).unwrap_or(0.0);
            let receive = it.get("receive").and_then(json_f64).unwrap_or(0.0);
            // buy: amount = THB locked, receive = coins to receive
            // sell: amount = coins locked, receive = THB to receive
            let (amount_base, amount_quote) = match side {
                Action::Buy => (
                    if receive > 0.0 {
                        receive
                    } else if price > 0.0 {
                        amount / price
                    } else {
                        0.0
                    },
                    amount,
                ),
                Action::Sell => (
                    amount,
                    if receive > 0.0 { receive } else { amount * price },
                ),
                Action::Hold => (0.0, 0.0),
            };
            let ts = it
                .get("ts")
                .and_then(|v| v.as_i64())
                .unwrap_or_else(|| Utc::now().timestamp_millis());
            let created_at = Utc
                .timestamp_millis_opt(ts)
                .single()
                .unwrap_or_else(Utc::now);
            out.push(OpenOrder {
                order_id,
                symbol: symbol.to_uppercase(),
                quote: quote.to_uppercase(),
                side,
                order_type: it
                    .get("type")
                    .and_then(|v| v.as_str())
                    .unwrap_or("limit")
                    .to_string(),
                price,
                amount_base,
                amount_quote,
                created_at,
            });
        }
        Ok(out)
    }

    async fn place_order(&self, req: &OrderRequest) -> DomainResult<OrderResult> {
        // Trading API is still v3 per official docs; v4 has wallet/fiat/crypto but no place order yet.
        // Resolve symbol from /api/v3/market/symbols then fallback to legacy format once if engine rejects.
        let meta = self.market_symbol(&req.symbol, &req.quote).await?;
        if meta.status != "active" {
            return Err(DomainError::Broker(format!(
                "trading pair {} is not active (status={})",
                meta.api_symbol, meta.status
            )));
        }
        // Bitkub "broker" coins are routed to a third-party broker and the order endpoint
        // rejects them with error 61. Catch it here with a clear message instead of firing a
        // doomed request (and the discovery universe filters these out up front anyway).
        if meta.source.eq_ignore_ascii_case("broker") {
            return Err(DomainError::Broker(format!(
                "{} is a Bitkub broker-listed coin and cannot be traded via API — only exchange-listed coins are supported",
                meta.api_symbol
            )));
        }

        let (path, amount, amount_scale) = match req.action {
            Action::Buy => {
                if meta.freeze_buy {
                    return Err(DomainError::Broker(format!(
                        "Bitkub has temporarily suspended buying {}",
                        meta.api_symbol
                    )));
                }
                if req.amount_quote < meta.min_quote_size {
                    return Err(DomainError::Broker(format!(
                        "buy amount {:.2} {} is below Bitkub minimum {:.2} {}",
                        req.amount_quote, req.quote, meta.min_quote_size, req.quote
                    )));
                }
                ("/api/v3/market/place-bid", req.amount_quote, 2)
            }
            Action::Sell => {
                if meta.freeze_sell {
                    return Err(DomainError::Broker(format!(
                        "Bitkub has temporarily suspended selling {}",
                        meta.api_symbol
                    )));
                }
                // Market sell: amt = coin quantity → must convert from quote to base using price
                let price = self
                    .last_price(&req.symbol, &req.quote)
                    .await
                    .unwrap_or(0.0);
                let amt_base = if price > 0.0 {
                    round_down(req.amount_quote / price, meta.base_asset_scale)
                } else {
                    0.0
                };
                if amt_base <= 0.0 {
                    return Err(DomainError::Broker("sell amount calculated to 0".into()));
                }
                ("/api/v3/market/place-ask", amt_base, meta.base_asset_scale)
            }
            Action::Hold => return Err(DomainError::Broker("HOLD does not require placing an order".into())),
        };
        let (result, used_symbol) = self
            .place_with_symbol_fallback(path, &meta, amount, amount_scale)
            .await?;
        let order_id = result
            .get("id")
            .and_then(|v| v.as_str().map(ToString::to_string))
            .or_else(|| result.get("id").map(|v| v.to_string()))
            .unwrap_or_default();
        let price = self
            .last_price(&req.symbol, &req.quote)
            .await
            .unwrap_or(0.0);
        let info = if order_id.is_empty() {
            json!({})
        } else {
            self.order_info(&used_symbol, &order_id, req.action)
                .await
                .unwrap_or_else(|e| {
                    tracing::warn!(
                        order_id = %order_id,
                        sym = %used_symbol,
                        "failed to read order-info after placing order: {e}"
                    );
                    json!({})
                })
        };
        let filled_quote = info
            .get("filled")
            .and_then(positive_json_f64)
            .or_else(|| result.get("filled").and_then(positive_json_f64))
            .unwrap_or(req.amount_quote);
        let filled_amount = match req.action {
            Action::Buy => result
                .get("rec")
                .or_else(|| result.get("receive"))
                .and_then(positive_json_f64)
                .or_else(|| {
                    if price > 0.0 && filled_quote > 0.0 {
                        Some(filled_quote / price)
                    } else {
                        None
                    }
                })
                .unwrap_or(0.0),
            Action::Sell => info
                .get("filled")
                .and_then(positive_json_f64)
                .or_else(|| result.get("amt").and_then(positive_json_f64))
                .unwrap_or(0.0),
            Action::Hold => 0.0,
        };
        Ok(OrderResult {
            broker: "bitkub".into(),
            order_id,
            symbol: req.symbol.clone(),
            action: req.action,
            filled_amount,
            price,
            simulated: false,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bitkub_number_trims_trailing_zeros() {
        assert_eq!(bitkub_number(150.0, 2).unwrap(), "150");
        assert_eq!(bitkub_number(150.5, 2).unwrap(), "150.5");
        assert_eq!(bitkub_number(0.12345678, 8).unwrap(), "0.12345678");
    }

    #[test]
    fn order_body_uses_base_quote_symbol_and_clean_numbers() {
        let body = bitkub_order_body("eth_thb", 150.0, 2).unwrap();
        assert_eq!(
            body,
            r#"{"sym":"eth_thb","amt":150,"rat":0,"typ":"market"}"#
        );
    }

    #[test]
    fn order_body_can_use_legacy_quote_base_symbol() {
        let body = bitkub_order_body("thb_eth", 150.0, 2).unwrap();
        assert_eq!(
            body,
            r#"{"sym":"thb_eth","amt":150,"rat":0,"typ":"market"}"#
        );
    }

    #[test]
    fn buy_result_uses_received_base_amount() {
        let result = json!({"amt": 150, "rec": 0.00281812});
        assert_eq!(result.get("rec").and_then(json_f64).unwrap(), 0.00281812);
    }

    #[test]
    fn positive_json_f64_ignores_zero_fills() {
        assert!(positive_json_f64(&json!(0)).is_none());
        assert_eq!(positive_json_f64(&json!("0.125")).unwrap(), 0.125);
    }

    #[test]
    fn serde_json_number_keeps_integer_body_possible() {
        let n = Value::Number(serde_json::Number::from(150));
        assert_eq!(n.to_string(), "150");
    }

    #[test]
    fn encode_cursor_escapes_base64_specials() {
        // standard base64 contains + / = which must be percent-encoded before inserting into query (and must sign the same string)
        assert_eq!(encode_cursor("ab+c/d=="), "ab%2Bc%2Fd%3D%3D");
        // base64url (-_) is already safe, no encoding needed
        assert_eq!(encode_cursor("ab-_C9.~"), "ab-_C9.~");
    }

    #[test]
    fn parse_history_buy_derives_base_from_quote_over_rate() {
        let it = json!({
            "side": "buy", "rate": "50000", "amount": "100",
            "order_id": "X1", "fee": "0.25", "ts": 1700000000000_i64
        });
        let o = parse_history_order(&it, "eth", "thb").unwrap();
        assert_eq!(o.side, Action::Buy);
        assert_eq!(o.amount_quote, 100.0);
        assert!((o.amount_base - 0.002).abs() < 1e-12); // 100 / 50000
        assert_eq!(o.symbol, "ETH");
    }

    #[test]
    fn parse_history_sell_uses_base_amount_directly() {
        let it = json!({
            "side": "sell", "rate": "50000", "amount": "0.01",
            "txn_id": "X2", "order_closed_at": 1700000000000_i64
        });
        let o = parse_history_order(&it, "eth", "thb").unwrap();
        assert_eq!(o.side, Action::Sell);
        assert!((o.amount_base - 0.01).abs() < 1e-12);
        assert!((o.amount_quote - 500.0).abs() < 1e-9); // 0.01 * 50000
    }

    #[test]
    fn parse_history_skips_incomplete_or_non_trade_rows() {
        assert!(parse_history_order(&json!({"side": "buy", "rate": "0", "amount": "100", "order_id": "Z"}), "eth", "thb").is_none());
        assert!(parse_history_order(&json!({"side": "deposit", "rate": "1", "amount": "1", "order_id": "Z"}), "eth", "thb").is_none());
        assert!(parse_history_order(&json!({"side": "buy", "rate": "1", "amount": "1"}), "eth", "thb").is_none()); // no order_id
    }
}
