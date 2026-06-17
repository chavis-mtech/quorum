//! Domain models — pure business core, no framework/DB/HTTP dependencies

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum Action {
    Buy,
    Sell,
    Hold,
}

impl Action {
    pub fn as_str(&self) -> &'static str {
        match self {
            Action::Buy => "BUY",
            Action::Sell => "SELL",
            Action::Hold => "HOLD",
        }
    }
    pub fn parse(s: &str) -> Action {
        match s.to_uppercase().as_str() {
            "BUY" => Action::Buy,
            "SELL" => Action::Sell,
            _ => Action::Hold,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum TradingMode {
    Paper,
    Live,
    SignalOnly,
}

/// Vote from a single AI advisor
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentVote {
    pub agent: String,
    pub action: Action,
    pub confidence: f64,
    pub reasoning: String,
    pub veto: bool,
    pub ok: bool,
}

/// Aggregated consensus from the aggregator
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Consensus {
    pub action: Action,
    pub confidence: f64,
    pub agreement: u32,
    pub voted: u32,
    pub vetoed: bool,
    pub passed_threshold: bool,
    pub reasoning: String,
    pub votes: Vec<AgentVote>,
}

/// Final verdict from the Judge LLM
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Verdict {
    pub action: Action,
    pub confidence: f64,
    pub reasoning: String,
    pub engine: String,
    #[serde(default)]
    pub suggested_size_pct: f64,
    /// LLM thinking process (qwen3 thinking) — shown to the user
    #[serde(default)]
    pub thinking: String,
    // ---- trade plan ----
    #[serde(default)]
    pub thesis: String,
    #[serde(default = "default_entry_type")]
    pub entry_type: String, // market | limit | none
    #[serde(default)]
    pub entry_price: f64,
    #[serde(default)]
    pub target_price: f64,
    #[serde(default)]
    pub stop_price: f64,
    #[serde(default)]
    pub invalidation: String,
    #[serde(default)]
    pub next_step: String,
}

fn default_entry_type() -> String {
    "market".into()
}

/// Full analysis result for 1 asset (what the AI engine returns)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Analysis {
    pub symbol: String,
    pub quote: String,
    pub last_price: Option<f64>,
    pub mode: String,
    pub data_source: String,
    pub synthetic: bool,
    pub news_source: String,
    pub news_count: u32,
    #[serde(default)]
    pub web_source: String,
    #[serde(default)]
    pub web_count: u32,
    pub consensus: Consensus,
    pub verdict: Verdict,
    /// reasoning trace — every step the AI thought/performed (displayed as a timeline on the UI)
    #[serde(default)]
    pub trace: Vec<serde_json::Value>,
}

/// Brief price data for an asset (for autocomplete + price details)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SymbolTicker {
    pub symbol: String,
    pub quote: String,
    pub last: f64,
    pub change_24h_pct: f64,
    pub high_24h: f64,
    pub low_24h: f64,
    pub volume_24h: f64,
}

/// Record of a single decision (stored in PostgreSQL for reporting)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DecisionRecord {
    pub id: i64,
    #[serde(default)]
    pub account_id: i64,
    pub symbol: String,
    pub quote: String,
    pub mode: TradingMode,
    pub final_action: Action,
    pub consensus_action: Action,
    pub consensus_confidence: f64,
    pub agreement: u32,
    pub voted: u32,
    pub vetoed: bool,
    pub judge_engine: String,
    pub judge_reasoning: String,
    pub last_price: Option<f64>,
    pub executed: bool,
    pub note: String,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

/// Broker credentials (entered via modal → stored in secret store)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BrokerCredentials {
    pub broker: String,
    pub api_key: String,
    pub api_secret: String,
}

/// Stored credential status — masked preview so the user can confirm it is the same key they set,
/// without exposing the full key/secret (only "head…tail" is sent) and shows when it was last updated
#[derive(Debug, Clone, Serialize)]
pub struct SecretMeta {
    /// masked api key e.g. "sk-or…3f9a" — lets the user verify the key without revealing it fully
    pub api_key_hint: String,
    /// masked tail of the secret e.g. "••••3f9a" — empty if no secret (cloud AI uses key only)
    pub api_secret_hint: String,
    pub has_secret: bool,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

// ---- Broker I/O ----

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrderRequest {
    pub symbol: String,
    pub quote: String,
    pub action: Action, // Buy/Sell
    pub amount_quote: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrderResult {
    pub broker: String,
    pub order_id: String,
    pub symbol: String,
    pub action: Action,
    pub filled_amount: f64,
    pub price: f64,
    pub simulated: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BrokerOrder {
    pub order_id: String,
    pub symbol: String,
    pub quote: String,
    pub side: Action,
    pub amount_base: f64,
    pub amount_quote: f64,
    pub price: f64,
    pub fee_quote: f64,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

/// Open order (limit order not yet matched) — funds/coins are locked, not yet in available balance
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpenOrder {
    pub order_id: String,
    pub symbol: String,
    pub quote: String,
    pub side: Action,
    pub order_type: String, // limit | market
    pub price: f64,
    /// coins to receive (buy) or locked for sale (sell)
    pub amount_base: f64,
    /// funds locked for purchase (buy) or to be received (sell)
    pub amount_quote: f64,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Balance {
    pub asset: String,
    pub available: f64,
}

// ---- Risk ----

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RiskConfig {
    pub max_position_pct: f64,
    pub daily_loss_limit: f64,
    pub max_open_positions: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PortfolioSnapshot {
    pub equity: f64,
    pub daily_pnl_pct: f64,
    pub open_positions: usize,
    pub cash_thb: f64,
    pub deployed_pct: f64,
    pub session_pnl_pct: f64,
}

/// Result from the risk layer (has authority to override the judge)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum RiskDecision {
    Allow { max_amount_quote: f64 },
    Block { reason: String },
}

// ---- Report ----

/// Summary for the report page (from aggregate in PostgreSQL)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReportSummary {
    pub total_decisions: i64,
    pub executed: i64,
    pub buy: i64,
    pub sell: i64,
    pub hold: i64,
    pub vetoed: i64,
    pub avg_confidence: f64,
    pub symbols_tracked: i64,
}

// ---- Trading settings (editable via UI, used by orchestrator) ----

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TradingSettings {
    pub mode: TradingMode,
    pub auto_trade: bool,
    pub trade_amount_quote: f64,
    pub max_position_pct: f64,
    pub min_confidence: f64,
    pub daily_loss_limit: f64,
    pub max_open_positions: i32,
    pub allow_sell: bool,
    pub take_profit_pct: f64,
    pub stop_loss_pct: f64,
    pub discovery_enabled: bool,
    pub discovery_top_n: i32,
    /// kill-switch — true = temporarily stop trading/analysis (user-triggered) [Phase 3]
    #[serde(default)]
    pub paused: bool,
    /// Judge LLM used for final decisions: local Ollama or cloud provider [Phase 5]
    #[serde(default = "default_ai_judge_enabled")]
    pub ai_judge_enabled: bool,
    #[serde(default = "default_ai_judge_provider")]
    pub ai_judge_provider: String,
    #[serde(default = "default_ai_judge_model")]
    pub ai_judge_model: String,
    #[serde(default = "default_ai_judge_ollama_url")]
    pub ai_judge_ollama_url: String,
    /// base URL for OpenAI-compatible provider (OpenAI/Groq/OpenRouter/custom)
    #[serde(default)]
    pub ai_judge_base_url: String,
    #[serde(default = "default_ai_judge_thinking")]
    pub ai_judge_thinking: bool,
    /// broker this account uses for live trading — "bitkub" (ready) | "binance" (scaffolded)
    #[serde(default = "default_broker")]
    pub broker: String,
    /// how aggressively to manage open positions — off | conservative | balanced | aggressive
    #[serde(default = "default_manage_style")]
    pub manage_style: String,
    /// when true, hitting the target tightens the trailing stop instead of a hard take-profit
    #[serde(default = "default_let_winners_run")]
    pub let_winners_run: bool,
}

fn default_manage_style() -> String {
    "conservative".into()
}
fn default_let_winners_run() -> bool {
    true
}

fn default_broker() -> String {
    "bitkub".into()
}

/// Known brokers — add new ones here only; resolver/factory selects based on settings value
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum BrokerKind {
    Bitkub,
    Binance,
}

impl BrokerKind {
    /// Lenient parse — unknown values fall back to Bitkub (the ready broker) to avoid trade failures
    pub fn parse(s: &str) -> Self {
        match s.trim().to_lowercase().as_str() {
            "binance" => BrokerKind::Binance,
            _ => BrokerKind::Bitkub,
        }
    }
    pub fn as_str(&self) -> &'static str {
        match self {
            BrokerKind::Bitkub => "bitkub",
            BrokerKind::Binance => "binance",
        }
    }
    /// Name of the secret storing this broker's API key (per user)
    pub fn secret_name(&self) -> &'static str {
        self.as_str()
    }
    /// Whether this broker can execute real orders (scaffolded but not yet open = false)
    pub fn is_ready(&self) -> bool {
        matches!(self, BrokerKind::Bitkub)
    }
}

fn default_ai_judge_enabled() -> bool {
    true
}
fn default_ai_judge_provider() -> String {
    "ollama".into()
}
fn default_ai_judge_model() -> String {
    "qwen3:14b".into()
}
fn default_ai_judge_ollama_url() -> String {
    "http://localhost:11434".into()
}
fn default_ai_judge_thinking() -> bool {
    true
}

// ---- Paper wallet (simulated wallet) ----

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PaperPosition {
    pub symbol: String,
    pub amount_base: f64,
    pub avg_price: f64,
    /// Latest market price (filled when calculating P&L)
    #[serde(default)]
    pub last_price: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WalletView {
    pub cash_quote: f64,
    pub starting_cash: f64,
    pub positions: Vec<PaperPosition>,
    pub positions_value: f64,
    pub equity: f64,
    pub pnl: f64,
    pub pnl_pct: f64,
    pub simulated: bool,
}

// ---- Trade record (linked to decision) ----

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TradeRecord {
    pub id: i64,
    #[serde(default)]
    pub account_id: i64,
    pub decision_id: Option<i64>,
    pub symbol: String,
    pub quote: String,
    pub side: Action,
    pub mode: TradingMode,
    pub simulated: bool,
    pub amount_base: f64,
    pub amount_quote: f64,
    pub price: f64,
    pub status: String,
    pub external_order_id: String,
    pub note: String,
    /// Realized gain/loss from this sell (paper) — buy/live = 0
    #[serde(default)]
    pub realized_pnl: f64,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

/// Result of a single simulated wallet fill
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FillResult {
    pub filled_base: f64,
    pub realized_pnl: f64,
}

/// Win/loss statistics (read from actual sells in the simulated wallet since session_start)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TradeStats {
    pub session_start: chrono::DateTime<chrono::Utc>,
    pub total_trades: i64,
    pub buys: i64,
    pub closed: i64, // number of sells that closed a position (have realized P&L)
    pub wins: i64,
    pub losses: i64,
    pub win_rate: f64,
    pub gross_pnl: f64,
    pub avg_win: f64,
    pub avg_loss: f64,
    pub best: f64,
    pub worst: f64,
    pub profit_factor: f64, // total profit / total loss
}

// ---- Trade plan (AI-created trade plan — tracked until the moment) ----

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PlanState {
    Pending, // waiting for price to reach entry
    Open,    // entered, waiting for target/stop
    Closed,  // closed
    Cancelled,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TradePlan {
    pub id: i64,
    #[serde(default)]
    pub account_id: i64,
    pub symbol: String,
    pub quote: String,
    pub state: PlanState,
    pub action: Action,     // intended direction (BUY/SELL)
    pub entry_type: String, // market | limit
    pub entry_price: f64,
    pub target_price: f64,
    pub stop_price: f64,
    pub confidence: f64,
    pub thesis: String,
    pub invalidation: String,
    pub next_step: String,
    pub decision_id: Option<i64>,
    pub last_price: f64,
    /// Peak price reached since entry — basis for the trailing stop (0 until first managed)
    #[serde(default)]
    pub high_water_mark: f64,
    /// Stop price set at entry; risk R = entry - initial_stop never changes (used for R-multiples)
    #[serde(default)]
    pub initial_stop: f64,
    /// True once breakeven/trailing management has moved the stop (UI badge + logic flag)
    #[serde(default)]
    pub trail_active: bool,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

// ---- Active position management (trailing stop / breakeven), Phase 4+ ----

/// Resolved management parameters for an account, expressed in R-multiples (R = entry - initial_stop).
/// Derived from the `manage_style` setting so the UI only exposes a single preset dropdown.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ManageCfg {
    /// trailing/breakeven disabled entirely (legacy static stop/target)
    pub enabled: bool,
    /// move stop to breakeven (risk-free) once unrealized profit reaches this many R
    pub breakeven_r: f64,
    /// start trailing the stop once unrealized profit reaches this many R
    pub activate_r: f64,
    /// keep the trailing stop this many R below the high-water mark
    pub trail_r: f64,
    /// extra buffer above entry when moving to breakeven, to cover round-trip fees (e.g. 0.005 = 0.5%)
    pub fee_buffer: f64,
    /// Fixed-% profit lock: once unrealized profit reaches this fraction of entry (e.g. 0.025 = +2.5%),
    /// the stop is lifted to breakeven+fees REGARDLESS of R. This stops a winning trade from
    /// round-tripping back into a loss when the R-based breakeven threshold is far away (wide stop).
    pub lock_profit_pct: f64,
}

impl ManageCfg {
    pub fn from_style(style: &str) -> Self {
        let (enabled, breakeven_r, activate_r, trail_r, lock_profit_pct) =
            match style.trim().to_lowercase().as_str() {
                "off" | "none" | "" => (false, 0.0, 0.0, 0.0, 0.0),
                "balanced" => (true, 1.0, 1.5, 1.5, 0.025),
                "aggressive" => (true, 1.5, 2.0, 2.5, 0.035),
                // "conservative" (default): lock profit early, trail tight
                _ => (true, 0.7, 1.0, 1.0, 0.02),
            };
        ManageCfg {
            enabled,
            breakeven_r,
            activate_r,
            trail_r,
            fee_buffer: 0.005,
            lock_profit_pct,
        }
    }
}

// ---- Capital & Risk Governor (Phase 3) — "what is happening right now and why" ----

/// Capital/risk governor state for an account — shows the user clearly what is happening
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GovernorState {
    pub account_id: i64,
    /// trading | scanning | full | halted | paused | manual | signal
    pub state: String,
    /// Human-readable reason — e.g. "Trading halted: daily loss limit of 5% reached"
    pub reason: String,
    pub cash: f64,
    pub equity: f64,
    pub daily_pnl_pct: f64,
    pub loss_limit: f64,
    /// 0..1 — how much of the loss quota has been used (loss / limit)
    pub loss_used: f64,
    pub open_positions: i64,
    pub max_open_positions: i64,
    pub open_slots: i64,
    /// how many more buys are possible (limited by open slots and cash/trade size)
    pub buys_remaining: i64,
    pub trade_amount: f64,
    pub auto_trade: bool,
    pub paused: bool,
    /// estimated number of assets that can be watched given this capital/risk budget
    pub watch_capacity: i64,
}

impl GovernorState {
    pub fn is_blocked(&self) -> bool {
        matches!(self.state.as_str(), "halted" | "paused")
    }
}

// ---- Target pipeline (Phase 4) — "targets being tracked + why not bought yet" ----

/// Status of each asset being watched — shows clearly what the AI thinks about this asset
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TargetStatus {
    pub symbol: String,
    /// holding | plan_pending | candidate | waiting | skipped | queued
    pub state: String,
    /// Human-readable reason e.g. "Waiting for price to drop to 2,450 (currently 2,500)"
    pub reason: String,
    pub last_price: f64,
    pub entry_price: f64,
    pub target_price: f64,
    pub stop_price: f64,
    pub confidence: f64,
    pub action: String,
    pub decision_id: Option<i64>,
    pub updated_at: Option<chrono::DateTime<chrono::Utc>>,
}

// ---- Market scan (AI self-discovers the market) ----

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MarketScanItem {
    pub symbol: String,
    pub score: f64,
    pub reason: String,
    pub last_price: f64,
    pub change_24h: f64,
}

// ---- Identity & accounts (multi-tenant) ----

/// System user (one person = multiple trading accounts)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct User {
    pub id: i64,
    pub email: String,
    pub display_name: String,
    pub role: String, // "user" | "admin"
    #[serde(skip_serializing)]
    pub password_hash: String, // never sent out via API
    pub created_at: chrono::DateTime<chrono::Utc>,
}

/// Safe user view for API responses (no hash)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserView {
    pub id: i64,
    pub email: String,
    pub display_name: String,
    pub role: String,
}

impl From<&User> for UserView {
    fn from(u: &User) -> Self {
        UserView {
            id: u.id,
            email: u.email.clone(),
            display_name: u.display_name.clone(),
            role: u.role.clone(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AccountKind {
    Paper,
    Live,
}

impl AccountKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            AccountKind::Paper => "paper",
            AccountKind::Live => "live",
        }
    }
    pub fn parse(s: &str) -> AccountKind {
        match s.to_lowercase().as_str() {
            "live" => AccountKind::Live,
            _ => AccountKind::Paper,
        }
    }
}

/// User trading account (paper simulated / live real money) — transactions are strictly isolated
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Account {
    pub id: i64,
    pub user_id: i64,
    pub kind: AccountKind,
    pub name: String,
    pub base_quote: String,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

/// Request context (who, which account) — attached to every handler that requires login
#[derive(Debug, Clone, Copy)]
pub struct Ctx {
    pub user_id: i64,
    pub account_id: i64,
    pub account_kind: AccountKind,
}

/// JWT payload
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Claims {
    pub sub: i64, // user id
    pub email: String,
    pub exp: i64, // unix expiry
    pub iat: i64,
}

// ---- Alert (events the user should know about — notified in UI + stored in DB) ----

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AlertRecord {
    pub id: i64,
    pub account_id: i64,
    /// info | warn | error
    pub level: String,
    /// machine code e.g. insufficient_funds, order_failed, plan_cancelled, rescue_plan
    pub code: String,
    pub message: String,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

// ---- Live event (pushed to UI via WebSocket) ----

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum LiveEvent {
    /// Indicates which asset is currently being analyzed
    Analyzing { account_id: i64, symbol: String },
    /// Analysis result + decision completed for 1 cycle
    Decision {
        record: DecisionRecord,
        analysis: Analysis,
    },
    /// A trade occurred (auto or manual)
    Trade { trade: TradeRecord },
    /// AI scan discovered interesting market opportunities
    Discovery { items: Vec<MarketScanItem> },
    /// Capital/risk governor state (Trading/Scanning/Halted/Paused)
    Governor { governor: GovernorState },
    /// Progress during analysis (percentage + thinking pieces) — UI sees real-time
    Progress {
        account_id: i64,
        symbol: String,
        /// 0–100
        pct: u8,
        /// stage: data | web | agent | consensus | judge
        stage: String,
        /// Human-readable stage description
        title: String,
        /// thinking added in this chunk (empty = stage advance only)
        #[serde(default)]
        thinking: String,
    },
    /// System status (engine/db/broker)
    Status { message: String, healthy: bool },
    /// Events the user should know about (insufficient funds, order failed, plan cancelled, etc.)
    Alert { alert: AlertRecord },
}

/// Single progress chunk from the AI sidecar (sidecar → backend → publish as LiveEvent::Progress)
#[derive(Debug, Clone, Default)]
pub struct AnalyzeProgress {
    pub pct: u8,
    pub stage: String,
    pub label: String,
    pub delta: String,
}

impl LiveEvent {
    /// The account this event belongs to — None = broadcast to everyone (market/system-wide)
    pub fn account_id(&self) -> Option<i64> {
        match self {
            LiveEvent::Analyzing { account_id, .. } => Some(*account_id),
            LiveEvent::Decision { record, .. } => Some(record.account_id),
            LiveEvent::Trade { trade } => Some(trade.account_id),
            LiveEvent::Governor { governor } => Some(governor.account_id),
            LiveEvent::Progress { account_id, .. } => Some(*account_id),
            LiveEvent::Alert { alert } => Some(alert.account_id),
            LiveEvent::Discovery { .. } | LiveEvent::Status { .. } => None,
        }
    }
}
