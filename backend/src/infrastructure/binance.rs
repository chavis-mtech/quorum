//! Broker adapter: Binance — scaffold ready for future use
//!
//! Current status:
//!   ✅ Public price (GET /api/v3/ticker/price) — ready to use
//!   🚧 Private endpoints (balance/order placement) — scaffold + clear message that not yet enabled
//!      When to enable: add HMAC signing (like Bitkub) + test against testnet first
//!      https://testnet.binance.vision — do not go live without passing testnet
//!
//! Note on quote: Binance has no THB pairs — USDT is the primary quote. The resolver/service
//! sends the quote per account; if it is THB, it will automatically map to USDT on the price side.

use async_trait::async_trait;
use serde_json::Value;
use tokio::sync::RwLock;

use crate::domain::models::{Balance, BrokerCredentials, OrderRequest, OrderResult};
use crate::domain::ports::{Broker, DomainError, DomainResult};

const BASE: &str = "https://api.binance.com";
const NOT_READY: &str =
    "Binance is not yet enabled — scaffold is ready, but signing + testnet testing must be completed before live trading (use Bitkub for now)";

pub struct BinanceBroker {
    #[allow(dead_code)] // will be used when private API is enabled
    creds: RwLock<Option<BrokerCredentials>>,
    http: reqwest::Client,
}

impl BinanceBroker {
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

    /// Binance trading pair format e.g. BTC+USDT → "BTCUSDT" — THB does not exist on Binance so it maps to USDT
    fn pair(symbol: &str, quote: &str) -> String {
        let quote = match quote.to_uppercase().as_str() {
            "THB" => "USDT".to_string(),
            q => q.to_string(),
        };
        format!("{}{}", symbol.to_uppercase(), quote)
    }
}

#[async_trait]
impl Broker for BinanceBroker {
    fn name(&self) -> &str {
        "binance"
    }
    fn is_simulated(&self) -> bool {
        false
    }

    async fn last_price(&self, symbol: &str, quote: &str) -> DomainResult<f64> {
        let pair = Self::pair(symbol, quote);
        let raw: Value = self
            .http
            .get(format!("{BASE}/api/v3/ticker/price"))
            .query(&[("symbol", pair.as_str())])
            .send()
            .await
            .map_err(|e| DomainError::Broker(e.to_string()))?
            .json()
            .await
            .map_err(|e| DomainError::Broker(e.to_string()))?;
        raw.get("price")
            .and_then(|v| v.as_str())
            .and_then(|s| s.parse::<f64>().ok())
            .filter(|p| *p > 0.0)
            .ok_or_else(|| DomainError::Broker(format!("price not found for {pair} on Binance")))
    }

    async fn balance(&self, _asset: &str) -> DomainResult<Balance> {
        Err(DomainError::Broker(NOT_READY.into()))
    }

    async fn balances(&self) -> DomainResult<Vec<Balance>> {
        Err(DomainError::Broker(NOT_READY.into()))
    }

    async fn place_order(&self, _req: &OrderRequest) -> DomainResult<OrderResult> {
        Err(DomainError::Broker(NOT_READY.into()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pair_maps_thb_to_usdt() {
        assert_eq!(BinanceBroker::pair("btc", "thb"), "BTCUSDT");
        assert_eq!(BinanceBroker::pair("ETH", "USDT"), "ETHUSDT");
        assert_eq!(BinanceBroker::pair("sol", "busd"), "SOLBUSD");
    }
}
