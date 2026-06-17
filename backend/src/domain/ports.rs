//! Ports — interfaces defined by domain/application for infrastructure to implement
//! (Dependency Inversion: inner layers have no knowledge of real DB/HTTP, only these traits)
//!
//! Post multi-tenant: every method that touches user data accepts a scope (account_id / user_id)

use async_trait::async_trait;

use super::models::{
    Account, AccountKind, AlertRecord, Analysis, AnalyzeProgress, Balance, BrokerCredentials,
    BrokerOrder, DecisionRecord, FillResult, MarketScanItem, OpenOrder, OrderRequest, OrderResult,
    PaperPosition, PlanState, ReportSummary, SecretMeta, SymbolTicker, TradePlan, TradeRecord,
    TradeStats, TradingSettings, User,
};

#[derive(Debug, thiserror::Error)]
pub enum DomainError {
    #[error("ai engine error: {0}")]
    Ai(String),
    #[error("broker error: {0}")]
    Broker(String),
    #[error("repository error: {0}")]
    Repo(String),
    #[error("secret store error: {0}")]
    Secret(String),
    #[error("blocked by risk: {0}")]
    RiskBlocked(String),
    #[error("not found: {0}")]
    NotFound(String),
    #[error("auth: {0}")]
    Auth(String),
}

pub type DomainResult<T> = Result<T, DomainError>;

/// AI analysis engine (panel of advisors + aggregator + judge) — typically a Python sidecar
#[async_trait]
pub trait AiEngine: Send + Sync {
    /// Analyze with a specified judge override (provider/model/key of the user) — for cloud AI [Phase 5]
    async fn analyze_with(
        &self,
        symbol: &str,
        judge_override: Option<serde_json::Value>,
    ) -> DomainResult<Analysis>;

    /// Analyze in "stream" mode — calls `on_progress` each time there is progress (percentage + thinking)
    /// default = no streaming (calls analyze_with) so engines that don't support it still work
    async fn analyze_stream(
        &self,
        symbol: &str,
        judge_override: Option<serde_json::Value>,
        on_progress: &(dyn Fn(AnalyzeProgress) + Send + Sync),
    ) -> DomainResult<Analysis> {
        let _ = on_progress;
        self.analyze_with(symbol, judge_override).await
    }

    async fn health(&self) -> DomainResult<()>;
}

/// Broker for prices/balances/order submission
#[async_trait]
pub trait Broker: Send + Sync {
    fn name(&self) -> &str;
    fn is_simulated(&self) -> bool;
    async fn last_price(&self, symbol: &str, quote: &str) -> DomainResult<f64>;
    async fn balance(&self, asset: &str) -> DomainResult<Balance>;
    /// All balances across every asset (for the live account page)
    async fn balances(&self) -> DomainResult<Vec<Balance>>;
    /// Matched order history from the real broker, used to reconcile DB/positions
    async fn order_history(
        &self,
        symbol: &str,
        quote: &str,
        limit: usize,
    ) -> DomainResult<Vec<BrokerOrder>> {
        let _ = (symbol, quote, limit);
        Ok(Vec::new())
    }
    /// Open orders (unmatched limit orders) for this trading pair — used to see locked funds/coins
    async fn open_orders(&self, symbol: &str, quote: &str) -> DomainResult<Vec<OpenOrder>> {
        let _ = (symbol, quote);
        Ok(Vec::new())
    }
    async fn place_order(&self, req: &OrderRequest) -> DomainResult<OrderResult>;
}

/// Resolves account_id → broker using the credentials of "the account owner"
/// (prevents one user from trading with another user's key in multi-tenant)
#[async_trait]
pub trait BrokerResolver: Send + Sync {
    async fn resolve(&self, account_id: i64) -> DomainResult<std::sync::Arc<dyn Broker>>;
}

/// User store (auth)
#[async_trait]
pub trait UserStore: Send + Sync {
    async fn count(&self) -> DomainResult<i64>;
    async fn create(
        &self,
        email: &str,
        password_hash: &str,
        display_name: &str,
        role: &str,
    ) -> DomainResult<User>;
    async fn by_email(&self, email: &str) -> DomainResult<Option<User>>;
    async fn by_id(&self, id: i64) -> DomainResult<Option<User>>;
    async fn update_profile(&self, id: i64, display_name: &str) -> DomainResult<()>;
    async fn update_password(&self, id: i64, password_hash: &str) -> DomainResult<()>;
}

/// Store for user trading accounts
#[async_trait]
pub trait AccountStore: Send + Sync {
    /// Create an account + seed settings (and wallet if paper)
    async fn create(&self, user_id: i64, kind: AccountKind, name: &str) -> DomainResult<Account>;
    async fn for_user(&self, user_id: i64) -> DomainResult<Vec<Account>>;
    async fn get(&self, account_id: i64) -> DomainResult<Option<Account>>;
    /// Delete an account (scoped by user_id to prevent deleting another user's account) — cascades to settings/wallet/trades/...
    async fn delete(&self, user_id: i64, account_id: i64) -> DomainResult<()>;
    /// Accounts with auto_trade enabled (for the watcher to monitor) — returns (account, kind)
    async fn auto_trading(&self) -> DomainResult<Vec<Account>>;
}

/// Trading settings store — per account
#[async_trait]
pub trait SettingsStore: Send + Sync {
    async fn get(&self, account_id: i64) -> DomainResult<TradingSettings>;
    async fn set(&self, account_id: i64, s: &TradingSettings) -> DomainResult<()>;
    async fn set_paused(&self, account_id: i64, paused: bool) -> DomainResult<()>;
}

/// Simulated (paper) wallet — per account
#[async_trait]
pub trait PaperWallet: Send + Sync {
    async fn view(&self, account_id: i64) -> DomainResult<(f64, f64, Vec<PaperPosition>)>;
    async fn reset(&self, account_id: i64, starting_cash: f64) -> DomainResult<()>;
    async fn apply_fill(
        &self,
        account_id: i64,
        symbol: &str,
        side_buy: bool,
        amount_quote: f64,
        price: f64,
    ) -> DomainResult<FillResult>;
    async fn position(&self, account_id: i64, symbol: &str) -> DomainResult<Option<PaperPosition>>;
    async fn session_start(&self, account_id: i64) -> DomainResult<chrono::DateTime<chrono::Utc>>;
    async fn reset_session(&self, account_id: i64) -> DomainResult<()>;
}

/// Trade history store — per account
#[async_trait]
pub trait TradeRepository: Send + Sync {
    async fn save(&self, t: &TradeRecord) -> DomainResult<i64>; // t.account_id is embedded
    async fn save_external(&self, t: &TradeRecord) -> DomainResult<i64>;
    async fn recent(&self, account_id: i64, limit: i64) -> DomainResult<Vec<TradeRecord>>;
    async fn for_symbol(
        &self,
        account_id: i64,
        symbol: &str,
        limit: i64,
    ) -> DomainResult<Vec<TradeRecord>>;
    /// Symbols that have filled trades but amount_base is still 0 (have an external_order_id) —
    /// used to fetch history from the broker to fill them in, even if the coins are no longer held
    async fn symbols_with_zero_amount(&self, account_id: i64) -> DomainResult<Vec<String>> {
        let _ = account_id;
        Ok(Vec::new())
    }
    /// Recompute realized P&L for all SELL trades of this symbol (walk the timeline, compute average cost) and write back —
    /// necessary because the broker (live) does not report profit/loss per order; used both when closing a trade and during backfill
    /// returns the number of rows whose values actually changed (idempotent — safe to call repeatedly)
    async fn recompute_realized(&self, account_id: i64, symbol: &str) -> DomainResult<u64> {
        let _ = (account_id, symbol);
        Ok(0)
    }
    /// All symbols this account has ever had filled trades for — used to backfill realized P&L for each one
    async fn distinct_symbols(&self, account_id: i64) -> DomainResult<Vec<String>> {
        let _ = account_id;
        Ok(Vec::new())
    }
    /// Currently-held amount and moving-average cost for (account, symbol) computed purely from the
    /// LOCAL filled-trade ledger — deterministic and independent of whatever the broker reports.
    /// Returns (amount_held, avg_cost); both 0.0 when there is no open local position.
    /// Used to record realized P&L at sell time so the dashboard always shows correct wins/losses.
    async fn position_basis(&self, account_id: i64, symbol: &str) -> DomainResult<(f64, f64)> {
        let _ = (account_id, symbol);
        Ok((0.0, 0.0))
    }
    async fn paper_stats(
        &self,
        account_id: i64,
        since: chrono::DateTime<chrono::Utc>,
    ) -> DomainResult<TradeStats>;
    /// Total realized profit/loss since the given time (all modes of the account) — used to compute loss limit
    async fn realized_since(
        &self,
        account_id: i64,
        since: chrono::DateTime<chrono::Utc>,
    ) -> DomainResult<f64>;
    async fn clear_all(&self, account_id: i64) -> DomainResult<u64>;
}

/// Market scanner to find interesting assets (AI-driven search)
#[async_trait]
pub trait MarketScanner: Send + Sync {
    async fn scan(&self, top_n: usize) -> DomainResult<Vec<MarketScanItem>>;
}

/// Watchlist store — per account
#[async_trait]
pub trait WatchlistStore: Send + Sync {
    async fn get_symbols(&self, account_id: i64) -> DomainResult<Vec<String>>;
    async fn set_symbols(&self, account_id: i64, symbols: &[String]) -> DomainResult<()>;
}

/// Trade plan store — per account (1 active plan per asset per account)
#[async_trait]
pub trait PlanRepository: Send + Sync {
    async fn upsert(&self, p: &TradePlan) -> DomainResult<i64>; // p.account_id is embedded
    async fn active(&self, account_id: i64) -> DomainResult<Vec<TradePlan>>;
    async fn get(&self, account_id: i64, symbol: &str) -> DomainResult<Option<TradePlan>>;
    async fn set_state(&self, account_id: i64, symbol: &str, state: PlanState) -> DomainResult<()>;
    async fn set_last_price(&self, account_id: i64, symbol: &str, price: f64) -> DomainResult<()>;

    /// Ratchet the trailing stop / high-water mark for an open position (monotonic — never lowers the stop).
    async fn update_trailing(
        &self,
        account_id: i64,
        symbol: &str,
        high_water: f64,
        new_stop: f64,
        trail_active: bool,
    ) -> DomainResult<()>;

    /// Re-analysis of a held position: update thesis/target/confidence and *raise* the stop
    /// (GREATEST, never lowers), while preserving high_water_mark / initial_stop / trail_active.
    #[allow(clippy::too_many_arguments)]
    async fn refresh_review(
        &self,
        account_id: i64,
        symbol: &str,
        target: f64,
        new_stop: f64,
        confidence: f64,
        thesis: &str,
        invalidation: &str,
        next_step: &str,
        decision_id: Option<i64>,
    ) -> DomainResult<()>;
}

/// Market/price data (autocomplete + price details) — separate from Broker (trading)
#[async_trait]
pub trait MarketData: Send + Sync {
    async fn search(&self, query: &str, limit: usize) -> DomainResult<Vec<SymbolTicker>>;
    async fn ticker(&self, symbol: &str) -> DomainResult<SymbolTicker>;
}

/// Decision history store (PostgreSQL) — per account
#[async_trait]
pub trait HistoryRepository: Send + Sync {
    async fn save_decision(&self, rec: &DecisionRecord) -> DomainResult<i64>; // rec.account_id is embedded
    async fn save_analysis_json(
        &self,
        decision_id: i64,
        raw: &serde_json::Value,
    ) -> DomainResult<()>;
    async fn recent_decisions(
        &self,
        account_id: i64,
        limit: i64,
    ) -> DomainResult<Vec<DecisionRecord>>;
    async fn decisions_for_symbol(
        &self,
        account_id: i64,
        symbol: &str,
        limit: i64,
    ) -> DomainResult<Vec<DecisionRecord>>;
    /// Full analysis payload (ownership verified by account_id)
    async fn decision_analysis(&self, account_id: i64, id: i64) -> DomainResult<serde_json::Value>;
    async fn report_summary(&self, account_id: i64) -> DomainResult<ReportSummary>;
}

/// Secret store (API keys for broker + AI cloud) — per user
#[async_trait]
pub trait SecretStore: Send + Sync {
    async fn get(&self, user_id: i64, name: &str) -> DomainResult<Option<BrokerCredentials>>;
    async fn set(&self, user_id: i64, creds: &BrokerCredentials) -> DomainResult<()>;
    async fn exists(&self, user_id: i64, name: &str) -> DomainResult<bool>;
    /// Masked preview + date set (None if never entered) — full key/secret is never returned
    async fn meta(&self, user_id: i64, name: &str) -> DomainResult<Option<SecretMeta>>;
}

/// Alert store (events the user should know about) — per account
#[async_trait]
pub trait AlertStore: Send + Sync {
    async fn save(&self, a: &AlertRecord) -> DomainResult<i64>;
    async fn recent(&self, account_id: i64, limit: i64) -> DomainResult<Vec<AlertRecord>>;
}

/// Channel for sending real-time updates to the UI (WebSocket)
pub trait EventSink: Send + Sync {
    fn publish(&self, event: &super::models::LiveEvent);
}
