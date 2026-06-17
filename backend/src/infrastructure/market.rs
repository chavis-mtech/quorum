//! MarketData adapter — price data from Bitkub public ticker (autocomplete + details)
//!
//! Calls GET /api/v3/market/ticker (all pairs) once, then caches briefly (~15s)
//! so autocomplete stays smooth without hitting Bitkub too frequently

use async_trait::async_trait;
use serde_json::Value;
use std::collections::HashSet;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use crate::domain::models::SymbolTicker;
use crate::domain::ports::{DomainError, DomainResult, MarketData};

const BASE: &str = "https://api.bitkub.com";
const TTL: Duration = Duration::from_secs(15);
/// Tradable-symbol list changes rarely → cache longer than the ticker
const SYMBOLS_TTL: Duration = Duration::from_secs(300);

pub struct BitkubMarket {
    quote: String,
    http: reqwest::Client,
    cache: Mutex<Option<(Instant, Vec<SymbolTicker>)>>,
    /// Set of base symbols (uppercase) that are actually tradable via the order API
    /// (source="exchange", status="active", buying not frozen) for this quote currency.
    /// Used to keep "broker" coins — which the order endpoint rejects with error 61 — out of
    /// the discovery universe so the bot never plans a trade it cannot execute.
    tradable: Mutex<Option<(Instant, HashSet<String>)>>,
}

impl BitkubMarket {
    pub fn new(quote: impl Into<String>) -> Self {
        Self {
            quote: quote.into(),
            http: reqwest::Client::builder()
                .timeout(Duration::from_secs(10))
                .build()
                .expect("http client"),
            cache: Mutex::new(None),
            tradable: Mutex::new(None),
        }
    }

    /// Parse /api/v3/market/symbols into the set of tradable base symbols for this quote.
    fn parse_tradable(&self, raw: &Value) -> HashSet<String> {
        let quote = self.quote.to_uppercase();
        let mut out = HashSet::new();
        let rows = raw.get("result").and_then(|v| v.as_array());
        if let Some(rows) = rows {
            for it in rows {
                let quote_ok = it
                    .get("quote_asset")
                    .and_then(|v| v.as_str())
                    .map(|s| s.eq_ignore_ascii_case(&quote))
                    .unwrap_or(false);
                if !quote_ok {
                    continue;
                }
                let source = it.get("source").and_then(|v| v.as_str()).unwrap_or("exchange");
                let status = it.get("status").and_then(|v| v.as_str()).unwrap_or("");
                let frozen = it.get("freeze_buy").and_then(|v| v.as_bool()).unwrap_or(false);
                if !source.eq_ignore_ascii_case("exchange") || status != "active" || frozen {
                    continue;
                }
                if let Some(base) = it.get("base_asset").and_then(|v| v.as_str()) {
                    out.insert(base.to_uppercase());
                }
            }
        }
        out
    }

    /// Tradable base-symbol set (cached). Fails OPEN: on a fetch error returns the last good
    /// set if any, else an empty set — and an empty set means "don't filter" in `search`, so a
    /// transient symbols-endpoint hiccup never empties the whole universe. The hard guarantee
    /// against trading a broker coin is the source check inside the broker's `place_order`.
    async fn tradable_set(&self) -> HashSet<String> {
        let cached = {
            let guard = self.tradable.lock().unwrap();
            guard.clone()
        };
        if let Some((t, ref set)) = cached {
            if t.elapsed() < SYMBOLS_TTL {
                return set.clone();
            }
        }
        let fetched = match self
            .http
            .get(format!("{BASE}/api/v3/market/symbols"))
            .send()
            .await
        {
            Ok(resp) => resp.json::<Value>().await.ok().map(|raw| self.parse_tradable(&raw)),
            Err(_) => None,
        };
        match fetched {
            Some(set) if !set.is_empty() => {
                *self.tradable.lock().unwrap() = Some((Instant::now(), set.clone()));
                set
            }
            // fetch failed or returned nothing usable → reuse last good set, else empty (= no filter)
            _ => cached.map(|(_, set)| set).unwrap_or_default(),
        }
    }

    fn parse_all(&self, raw: &Value) -> Vec<SymbolTicker> {
        let suffix = format!("_{}", self.quote.to_uppercase());
        let mut out = Vec::new();
        if let Some(arr) = raw.as_array() {
            for v in arr {
                let pair = v.get("symbol").and_then(|x| x.as_str()).unwrap_or("");
                if !pair.ends_with(&suffix) {
                    continue;
                }
                let symbol = pair[..pair.len() - suffix.len()].to_string();
                let f = |k: &str| {
                    v.get(k)
                        .and_then(|x| {
                            x.as_f64()
                                .or_else(|| x.as_str().and_then(|s| s.parse().ok()))
                        })
                        .unwrap_or(0.0)
                };
                out.push(SymbolTicker {
                    symbol,
                    quote: self.quote.to_uppercase(),
                    last: f("last"),
                    change_24h_pct: f("percent_change"),
                    high_24h: f("high_24_hr"),
                    low_24h: f("low_24_hr"),
                    volume_24h: f("quote_volume"),
                });
            }
        } else if let Some(map) = raw.as_object() {
            let prefix = format!("{}_", self.quote.to_uppercase());
            for (pair, v) in map {
                if !pair.starts_with(&prefix) {
                    continue;
                }
                let symbol = pair[prefix.len()..].to_string();
                let f = |k: &str| v.get(k).and_then(|x| x.as_f64()).unwrap_or(0.0);
                out.push(SymbolTicker {
                    symbol,
                    quote: self.quote.to_uppercase(),
                    last: f("last"),
                    change_24h_pct: f("percentChange"),
                    high_24h: f("high24hr"),
                    low_24h: f("low24hr"),
                    volume_24h: f("baseVolume"),
                });
            }
        }
        // sort by volume (high→low) so popular symbols appear first
        out.sort_by(|a, b| {
            b.volume_24h
                .partial_cmp(&a.volume_24h)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        out
    }

    async fn all(&self) -> DomainResult<Vec<SymbolTicker>> {
        if let Some((t, ref data)) = *self.cache.lock().unwrap() {
            if t.elapsed() < TTL {
                return Ok(data.clone());
            }
        }
        let raw: Value = self
            .http
            .get(format!("{BASE}/api/v3/market/ticker"))
            .send()
            .await
            .map_err(|e| DomainError::Broker(e.to_string()))?
            .json()
            .await
            .map_err(|e| DomainError::Broker(e.to_string()))?;
        let tickers = self.parse_all(&raw);
        *self.cache.lock().unwrap() = Some((Instant::now(), tickers.clone()));
        Ok(tickers)
    }
}

#[async_trait]
impl MarketData for BitkubMarket {
    async fn search(&self, query: &str, limit: usize) -> DomainResult<Vec<SymbolTicker>> {
        let q = query.trim().to_uppercase();
        let all = self.all().await?;
        let tradable = self.tradable_set().await;
        let filtered: Vec<SymbolTicker> = all
            .into_iter()
            .filter(|t| q.is_empty() || t.symbol.contains(&q))
            // keep only coins tradable via the order API (excludes Bitkub "broker" coins that
            // reject orders with error 61). Empty set = symbols fetch unavailable → don't filter.
            .filter(|t| tradable.is_empty() || tradable.contains(&t.symbol))
            .take(limit)
            .collect();
        Ok(filtered)
    }

    async fn ticker(&self, symbol: &str) -> DomainResult<SymbolTicker> {
        let s = symbol.trim().to_uppercase();
        self.all()
            .await?
            .into_iter()
            .find(|t| t.symbol == s)
            .ok_or_else(|| DomainError::NotFound(format!("asset not found: {s}")))
    }
}
