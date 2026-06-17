//! MarketScanner — AI self-discovers markets: ranks coins that are "interesting" right now
//!
//! Criteria: momentum (|%24h|) × liquidity (volume) — strong movers + high trading activity
//! = worth watching / opportunity hunting. Uses data from Bitkub ticker (live)

use async_trait::async_trait;
use std::sync::Arc;

use crate::domain::models::MarketScanItem;
use crate::domain::ports::{DomainResult, MarketData, MarketScanner};

pub struct MomentumScanner {
    market: Arc<dyn MarketData>,
}

impl MomentumScanner {
    pub fn new(market: Arc<dyn MarketData>) -> Self {
        Self { market }
    }
}

#[async_trait]
impl MarketScanner for MomentumScanner {
    async fn scan(&self, top_n: usize) -> DomainResult<Vec<MarketScanItem>> {
        // fetch all coins (empty query = all)
        let all = self.market.search("", 1000).await?;
        // normalize volume to log so that extremely high-volume coins don't dominate
        let mut scored: Vec<MarketScanItem> = all
            .into_iter()
            .filter(|t| t.last > 0.0 && t.volume_24h > 0.0)
            .map(|t| {
                let momentum = t.change_24h_pct.abs();
                let liquidity = (t.volume_24h.max(1.0)).ln();
                let score = momentum * liquidity;
                let dir = if t.change_24h_pct >= 0.0 {
                    "surging up"
                } else {
                    "falling down"
                };
                MarketScanItem {
                    reason: format!(
                        "{dir} {:.1}% in 24h, high liquidity (vol {:.0})",
                        t.change_24h_pct, t.volume_24h
                    ),
                    symbol: t.symbol,
                    score,
                    last_price: t.last,
                    change_24h: t.change_24h_pct,
                }
            })
            .collect();
        scored.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        scored.truncate(top_n);
        Ok(scored)
    }
}
