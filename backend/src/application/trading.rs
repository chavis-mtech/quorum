//! TradingService — thesis-driven trading engine, per account (multi-tenant)
//!
//! Concept: separate "thinking" (deep analysis, infrequent) from "watching" (monitor, frequent/lightweight)
//!   - run_once()       = deep analysis → place/adjust a "trade plan" (enter immediately or wait for entry)
//!   - check_triggers() = watch price; if entry level is reached → act (cheap, no AI call)
//!   - check_exits()    = watch price; if plan target/stop is reached → close position
//!
//! Every method accepts `account_id` to isolate data/actions per account

use std::sync::Arc;

use chrono::{Duration as ChronoDuration, Utc};
use serde_json::json;

use super::preflight::{self, BuyPreflight};
use super::risk;
use crate::domain::models::{
    AccountKind, Action, AlertRecord, Analysis, AnalyzeProgress, BrokerOrder, DecisionRecord,
    LiveEvent, ManageCfg, OpenOrder, OrderRequest, PaperPosition, PlanState, PortfolioSnapshot,
    RiskConfig, RiskDecision, TradePlan, TradeRecord, TradingMode, TradingSettings, WalletView,
};
use crate::domain::ports::{
    AccountStore, AiEngine, AlertStore, Broker, BrokerResolver, DomainError, DomainResult,
    EventSink, HistoryRepository, PaperWallet, PlanRepository, SecretStore, SettingsStore,
    TradeRepository,
};

const QUOTE: &str = "THB";
const MIN_REWARD_RISK: f64 = 1.35;
const MAX_PENDING_PLAN_AGE_HOURS: i64 = 12;
/// A plan that reaches its entry shortly after the analysis is still the same signal. Re-running
/// a non-deterministic LLM seconds later made valid BUY plans flip to HOLD and cancelled every
/// order. Fresh plans execute from the already-validated persisted levels; older plans still get
/// a full confirmation analysis before risking capital.
const FRESH_PLAN_DIRECT_EXEC_MINUTES: i64 = 30;
/// Limit entry price must not be more than this fraction below market — prevents "dead plans" waiting for a
/// distant pullback that will never trigger (e.g. waiting for −31%). If farther than this, skip plan and wait for a closer setup instead
const MAX_ENTRY_DISTANCE_PCT: f64 = 0.05;
/// A limit at/above the current market is marketable, not a real pullback. Treat tiny rounding
/// differences (up to 0.1%) as immediate entry rather than creating a plan that triggers seconds later.
const MARKETABLE_LIMIT_TOLERANCE_PCT: f64 = 0.001;
const RESCUE_TAKE_PROFIT_PCT: f64 = 0.04;
const RESCUE_STOP_LOSS_PCT: f64 = 0.025;

// ─── Hard risk caps (always on, independent of the plan's own stop) ─────────────
// Catastrophic per-trade loss cap: no single position is ever allowed to lose more than this,
// even if the AI set a wider stop or no stop at all. The effective exit stop is floored at
// avg_cost*(1-MAX_LOSS_PCT) so a definite loss is always cut.
// Tightened from 0.06: live data (account 4, 73 trades) showed avg loss -9.33 vs avg win +5.11
// per trade despite a 53% win rate — the 6% cap plus 60s poll interval let thin-liquidity market
// sells (AAVE/AERO/ID/AXL) slip past the intended stop to ~10.6% before the watch loop caught up.
// NOTE: there is intentionally NO time-based ("stale") exit — the user is fine holding through
// long drawdowns that eventually recover; only a real stop-out or a broken thesis closes a trade.
const MAX_LOSS_PCT: f64 = 0.05;

pub fn normalize_ai_provider(provider: &str) -> String {
    match provider.trim().to_lowercase().as_str() {
        "" => "ollama".into(),
        "claude" => "anthropic".into(),
        "openai-compatible" | "openai compatible" => "openai_compatible".into(),
        "ollama" | "openai" | "anthropic" | "groq" | "openrouter" | "openai_compatible"
        | "custom" | "none" => provider.trim().to_lowercase().replace('-', "_"),
        _ => "ollama".into(),
    }
}

pub fn ai_provider_needs_key(provider: &str) -> bool {
    !matches!(normalize_ai_provider(provider).as_str(), "ollama" | "none")
}

pub fn ai_base_url(provider: &str, configured: &str) -> String {
    let configured = configured.trim();
    if !configured.is_empty() {
        return configured.trim_end_matches('/').into();
    }
    match normalize_ai_provider(provider).as_str() {
        "groq" => "https://api.groq.com/openai/v1".into(),
        "openrouter" => "https://openrouter.ai/api/v1".into(),
        "openai" | "openai_compatible" => "https://api.openai.com/v1".into(),
        _ => String::new(),
    }
}

fn reward_risk(entry: f64, target: f64, stop: f64) -> Option<f64> {
    if entry <= 0.0 || target <= entry || stop <= 0.0 || stop >= entry {
        return None;
    }
    let risk = entry - stop;
    let reward = target - entry;
    if risk <= 0.0 {
        None
    } else {
        Some(reward / risk)
    }
}

fn validate_long_plan(entry: f64, target: f64, stop: f64) -> Result<f64, String> {
    match reward_risk(entry, target, stop) {
        Some(rr) if rr >= MIN_REWARD_RISK => Ok(rr),
        Some(rr) => Err(format!(
            "reward:risk {rr:.2} below threshold {MIN_REWARD_RISK:.2}"
        )),
        None => Err("entry/target/stop levels are not valid for a long plan".into()),
    }
}

fn should_enter_immediately(
    entry_type: &str,
    entry_price: f64,
    market_price: f64,
    confidence: f64,
    min_confidence: f64,
) -> bool {
    if confidence < min_confidence {
        return false;
    }
    entry_type == "market"
        || entry_price <= 0.0
        || (entry_type == "limit"
            && market_price > 0.0
            && entry_price >= market_price * (1.0 - MARKETABLE_LIMIT_TOLERANCE_PCT))
}

/// Compute the new (ratcheted) stop for an open long position using R-multiple management.
///
/// `R = basis - initial_stop` is the risk taken at entry (falls back to 2% of basis if the initial
/// stop is missing). Once unrealized profit clears `breakeven_r` the stop lifts to entry + fee buffer
/// (risk-free); once it clears `activate_r` the stop trails `trail_r * R` below the high-water mark.
///
/// Returns `(new_stop, trail_active)`. The stop is **monotonic** — it is never lowered below
/// `current_stop`, and never placed at/above the current price.
fn managed_stop(
    basis: f64,        // cost basis (position average price)
    price: f64,        // current market price
    high_water: f64,   // peak price seen since entry (>= price)
    initial_stop: f64, // stop price set at entry
    current_stop: f64, // current stop (may already be trailed)
    cfg: ManageCfg,
) -> (f64, bool) {
    if !cfg.enabled || basis <= 0.0 || price <= 0.0 {
        return (current_stop, false);
    }
    let r = if initial_stop > 0.0 && basis > initial_stop {
        basis - initial_stop
    } else {
        basis * 0.02 // no usable initial stop → assume 2% risk so management still functions
    };
    if r <= 0.0 {
        return (current_stop, false);
    }
    let profit_r = (price - basis) / r;
    let high_water = high_water.max(price);
    let mut new_stop = current_stop;

    // fixed-% profit lock: once price is far enough above entry, make the trade risk-free
    // regardless of R. This is what stops a winner from sliding back into a loss when the
    // R-based breakeven is far away (e.g. a wide initial stop). Independent of breakeven_r.
    if cfg.lock_profit_pct > 0.0 && price >= basis * (1.0 + cfg.lock_profit_pct) {
        new_stop = new_stop.max(basis * (1.0 + cfg.fee_buffer));
    }
    // breakeven: lift the stop to entry + fee buffer (truly risk-free) once profit clears the threshold
    if profit_r >= cfg.breakeven_r {
        new_stop = new_stop.max(basis * (1.0 + cfg.fee_buffer));
    }
    // trailing: keep the stop trail_r below the high-water mark once profit clears the activation threshold
    if profit_r >= cfg.activate_r {
        new_stop = new_stop.max(high_water - cfg.trail_r * r);
    }
    // never place the stop at/above the current price (would force an instant exit)
    new_stop = new_stop.min(price * 0.999);
    // monotonic: never lower an existing stop
    new_stop = new_stop.max(current_stop);

    let active = new_stop >= basis * 0.999 || profit_r >= cfg.activate_r;
    (new_stop, active)
}

pub struct TradingService {
    pub ai: Arc<dyn AiEngine>,
    /// broker for "price" (public, no key required) — shared across all accounts
    pub live_broker: Arc<dyn Broker>,
    /// resolver that binds a broker to the account owner's credentials (for live orders/balance)
    pub broker_resolver: Arc<dyn BrokerResolver>,
    pub repo: Arc<dyn HistoryRepository>,
    pub trades: Arc<dyn TradeRepository>,
    pub wallet: Arc<dyn PaperWallet>,
    pub plans: Arc<dyn PlanRepository>,
    pub settings: Arc<dyn SettingsStore>,
    pub secrets: Arc<dyn SecretStore>,
    pub accounts: Arc<dyn AccountStore>,
    pub events: Arc<dyn EventSink>,
    pub alerts: Arc<dyn AlertStore>,
}

impl TradingService {
    /// Notify the user (WS toast) + save to DB + log — used for all events the user should know about
    /// level: info | warn | error
    pub async fn alert(&self, account_id: i64, level: &str, code: &str, message: String) {
        match level {
            "error" => tracing::error!(account_id, code, "{message}"),
            "warn" => tracing::warn!(account_id, code, "{message}"),
            _ => tracing::info!(account_id, code, "{message}"),
        }
        let mut rec = AlertRecord {
            id: 0,
            account_id,
            level: level.to_string(),
            code: code.to_string(),
            message,
            created_at: Utc::now(),
        };
        match self.alerts.save(&rec).await {
            Ok(id) => rec.id = id,
            Err(e) => tracing::warn!("failed to save alert to DB: {e}"),
        }
        self.events.publish(&LiveEvent::Alert { alert: rec });
    }

    // ---------------- DEEP: analyze + plan ----------------

    pub async fn run_once(&self, account_id: i64, symbol: &str) -> DomainResult<DecisionRecord> {
        self.events.publish(&LiveEvent::Analyzing {
            account_id,
            symbol: symbol.to_string(),
        });
        let settings = self
            .settings
            .get(account_id)
            .await
            .unwrap_or_else(|_| default_settings());
        let analysis = self
            .analyze_streaming_for_account(account_id, symbol, &settings)
            .await?;

        let record = self.to_record(account_id, &analysis, settings.mode);
        let id = self.repo.save_decision(&record).await?;
        self.repo
            .save_analysis_json(id, &serde_json::to_value(&analysis).unwrap_or(json!({})))
            .await
            .ok();
        let record = DecisionRecord { id, ..record };
        self.events.publish(&LiveEvent::Decision {
            record: record.clone(),
            analysis: analysis.clone(),
        });

        // Place/adjust plan when auto-trade is on; paused still allows plan analysis
        // but every point that sends a real order must also check paused
        if settings.auto_trade
            && !matches!(settings.mode, TradingMode::SignalOnly)
            && !analysis.synthetic
        {
            if let Err(e) = self
                .plan_from_analysis(account_id, &analysis, &settings, id)
                .await
            {
                tracing::warn!("planning {symbol} failed: {e}");
            }
        }
        Ok(record)
    }

    pub fn local_judge_override(settings: &TradingSettings) -> serde_json::Value {
        let model = if normalize_ai_provider(&settings.ai_judge_provider) == "ollama" {
            settings.ai_judge_model.clone()
        } else {
            "qwen3:14b".into()
        };
        json!({
            "enabled": true,
            "provider": "ollama",
            "model": model,
            "ollama_url": &settings.ai_judge_ollama_url,
            "thinking": settings.ai_judge_thinking,
            "fallback": ["none"],
        })
    }

    pub async fn judge_override_for_account(
        &self,
        account_id: i64,
        symbol: &str,
        settings: &TradingSettings,
    ) -> Option<serde_json::Value> {
        if !settings.ai_judge_enabled {
            return Some(json!({ "enabled": false }));
        }

        let provider = normalize_ai_provider(&settings.ai_judge_provider);
        if provider == "none" {
            return Some(json!({ "enabled": true, "provider": "none" }));
        }

        let mut cfg = json!({
            "enabled": true,
            "provider": &provider,
            "model": &settings.ai_judge_model,
            "ollama_url": &settings.ai_judge_ollama_url,
            "thinking": settings.ai_judge_thinking,
            "fallback": if provider == "ollama" { json!(["none"]) } else { json!(["ollama", "none"]) },
        });

        if provider != "ollama" {
            cfg["base_url"] = json!(ai_base_url(&provider, &settings.ai_judge_base_url));
            if let Some(key) = self.ai_api_key(account_id, &provider).await {
                cfg["api_key"] = json!(key);
            }
        }
        if let Some(pos) = self
            .position_context_for_ai(account_id, symbol, settings)
            .await
        {
            cfg["position_context"] = pos;
        }
        // portfolio context — session P&L + deployed capital (so judge can adjust size)
        let pf = self.portfolio_snapshot(account_id, settings.mode, QUOTE).await;
        cfg["portfolio"] = json!({
            "session_pnl_pct": (pf.session_pnl_pct * 100.0 * 10.0).round() / 10.0,
            "loss_limit_pct": (settings.daily_loss_limit * 100.0 * 10.0).round() / 10.0,
            "deployed_pct":    (pf.deployed_pct * 100.0 * 10.0).round() / 10.0,
            "cash_thb":        (pf.cash_thb * 100.0).round() / 100.0,
            "equity":          (pf.equity * 100.0).round() / 100.0,
            "open_positions":  pf.open_positions,
        });
        Some(cfg)
    }

    async fn analyze_for_account(
        &self,
        account_id: i64,
        symbol: &str,
        settings: &TradingSettings,
    ) -> DomainResult<Analysis> {
        let judge = self
            .judge_override_for_account(account_id, symbol, settings)
            .await;
        self.ai.analyze_with(symbol, judge).await
    }

    /// Same as analyze_for_account but "streaming" — pushes LiveEvent::Progress (percent + thinking)
    /// so the UI can see incremental updates while the AI is reasoning
    async fn analyze_streaming_for_account(
        &self,
        account_id: i64,
        symbol: &str,
        settings: &TradingSettings,
    ) -> DomainResult<Analysis> {
        let judge = self
            .judge_override_for_account(account_id, symbol, settings)
            .await;
        let events = self.events.clone();
        let sym = symbol.to_string();
        let on = move |p: AnalyzeProgress| {
            events.publish(&LiveEvent::Progress {
                account_id,
                symbol: sym.clone(),
                pct: p.pct,
                stage: p.stage,
                title: p.label,
                thinking: p.delta,
            });
        };
        self.ai.analyze_stream(symbol, judge, &on).await
    }

    async fn ai_api_key(&self, account_id: i64, provider: &str) -> Option<String> {
        let account = self.accounts.get(account_id).await.ok().flatten()?;
        let secret_name = format!("ai:{provider}");
        self.secrets
            .get(account.user_id, &secret_name)
            .await
            .ok()
            .flatten()
            .map(|c| c.api_key)
            .filter(|k| !k.trim().is_empty())
    }

    async fn position_context_for_ai(
        &self,
        account_id: i64,
        symbol: &str,
        settings: &TradingSettings,
    ) -> Option<serde_json::Value> {
        let symbol = symbol.to_uppercase();
        let pos = self.current_position(account_id, &symbol, settings).await?;
        if pos.amount_base <= 0.0 || pos.avg_price <= 0.0 {
            return None;
        }
        let last = if pos.last_price > 0.0 {
            pos.last_price
        } else {
            self.live_broker
                .last_price(&symbol, QUOTE)
                .await
                .unwrap_or(pos.avg_price)
        };
        let pnl_pct = if pos.avg_price > 0.0 {
            (last / pos.avg_price - 1.0) * 100.0
        } else {
            0.0
        };
        // include the current managed plan levels so the judge can reason about the live trade:
        // how far profit has run in R-multiples, and where the (possibly trailed) stop/target sit
        let plan = self.plans.get(account_id, &symbol).await.ok().flatten();
        let (stop, target, profit_r) = match &plan {
            Some(p) => {
                let r = if p.initial_stop > 0.0 && pos.avg_price > p.initial_stop {
                    pos.avg_price - p.initial_stop
                } else {
                    pos.avg_price * 0.02
                };
                let pr = if r > 0.0 { (last - pos.avg_price) / r } else { 0.0 };
                (p.stop_price, p.target_price, pr)
            }
            None => (0.0, 0.0, 0.0),
        };
        Some(json!({
            "symbol": symbol,
            "amount": pos.amount_base,
            "avg_price": pos.avg_price,
            "last_price": last,
            "value_quote": pos.amount_base * last,
            "cost_quote": pos.amount_base * pos.avg_price,
            "pnl_pct": pnl_pct,
            "stop": stop,
            "target": target,
            "profit_r": (profit_r * 100.0).round() / 100.0,
        }))
    }

    async fn current_position(
        &self,
        account_id: i64,
        symbol: &str,
        settings: &TradingSettings,
    ) -> Option<PaperPosition> {
        self.current_position_result(account_id, symbol, settings)
            .await
            .ok()
            .flatten()
    }

    async fn uses_live_broker(&self, account_id: i64, settings: &TradingSettings) -> bool {
        matches!(settings.mode, TradingMode::Live) || self.is_live_account(account_id).await
    }

    async fn current_position_result(
        &self,
        account_id: i64,
        symbol: &str,
        settings: &TradingSettings,
    ) -> DomainResult<Option<PaperPosition>> {
        let symbol = symbol.to_uppercase();
        if self.uses_live_broker(account_id, settings).await {
            let (_, positions) = self.live_positions_from_broker(account_id, QUOTE).await?;
            Ok(positions
                .into_iter()
                .find(|p| p.symbol.eq_ignore_ascii_case(&symbol)))
        } else {
            self.wallet.position(account_id, &symbol).await
        }
    }

    /// Convert analysis result into a "plan" + execute immediately or set pending based on timing
    async fn plan_from_analysis(
        &self,
        account_id: i64,
        a: &Analysis,
        s: &TradingSettings,
        decision_id: i64,
    ) -> DomainResult<()> {
        let v = &a.verdict;
        let price = a.last_price.unwrap_or(
            self.live_broker
                .last_price(&a.symbol, QUOTE)
                .await
                .unwrap_or(0.0),
        );
        if price <= 0.0 {
            return Ok(());
        }
        let uses_live_broker = self.uses_live_broker(account_id, s).await;
        let current_position = self
            .current_position_result(account_id, &a.symbol, s)
            .await?;
        let holding = current_position
            .as_ref()
            .map(|p| p.amount_base > 0.0)
            .unwrap_or(false);

        // ----- holding position: review → exit if thesis reverses / adjust targets -----
        if holding {
            if uses_live_broker {
                if let Some(pos) = current_position.as_ref() {
                    self.ensure_live_rescue_plans(account_id, s, std::slice::from_ref(pos))
                        .await;
                }
            }
            if !s.paused
                && s.allow_sell
                && v.action == Action::Sell
                && v.confidence >= s.min_confidence
            {
                self.exit_position(
                    account_id,
                    &a.symbol,
                    "judge ordered exit (thesis reversed)",
                    Some(decision_id),
                )
                .await;
                self.plans
                    .set_state(account_id, &a.symbol, PlanState::Closed)
                    .await
                    .ok();
            } else if v.action == Action::Sell {
                tracing::info!(
                    "not selling {}: paused/allow_sell/confidence not yet permitting a real order",
                    a.symbol
                );
            } else {
                match self.validate_trackable_plan(a, s, price) {
                    Ok(_) => {
                        let existing =
                            self.plans.get(account_id, &a.symbol).await.ok().flatten();
                        let is_open = existing
                            .as_ref()
                            .map(|p| matches!(p.state, PlanState::Open))
                            .unwrap_or(false);
                        if is_open {
                            // refresh thesis/target and only ever raise the stop — keep the
                            // trailing state (high-water/initial_stop/trail_active) intact
                            let (_entry, target, stop) = self.plan_levels(a, price, s);
                            self.plans
                                .refresh_review(
                                    account_id,
                                    &a.symbol,
                                    target,
                                    stop,
                                    v.confidence,
                                    &v.thesis,
                                    &v.invalidation,
                                    &v.next_step,
                                    Some(decision_id),
                                )
                                .await
                                .ok();
                        } else {
                            // held but no open plan yet (e.g. reconciled live position) → create one
                            let plan = self.build_plan(
                                account_id,
                                a,
                                PlanState::Open,
                                Action::Buy,
                                decision_id,
                                price,
                                s,
                            );
                            self.plans.upsert(&plan).await.ok();
                        }
                    }
                    Err(reason) => tracing::info!("not updating plan for {}: {reason}", a.symbol),
                }
            }
            return Ok(());
        }

        // ----- not holding: decide entry timing -----
        match v.action {
            Action::Buy => {
                let enter_now = should_enter_immediately(
                    &v.entry_type,
                    v.entry_price,
                    price,
                    v.confidence,
                    s.min_confidence,
                );
                if enter_now {
                    self.enter_now(account_id, a, s, decision_id, price).await;
                } else if v.entry_price > 0.0 {
                    self.track_pending(account_id, a, s, decision_id, price)
                        .await?;
                }
            }
            _ => {
                // HOLD/SELL must never create a buy plan even if an LLM returned leftover entry
                // levels in its JSON. Only the explicit BUY branch above may schedule an entry.
                if let Ok(Some(p)) = self.plans.get(account_id, &a.symbol).await {
                    if matches!(p.state, PlanState::Pending) {
                        self.plans
                            .set_state(account_id, &a.symbol, PlanState::Cancelled)
                            .await
                            .ok();
                    }
                }
            }
        }
        Ok(())
    }

    async fn track_pending(
        &self,
        account_id: i64,
        a: &Analysis,
        s: &TradingSettings,
        decision_id: i64,
        price: f64,
    ) -> DomainResult<()> {
        // Prevent dead plans: the entry level requested by judge must be close to market, not waiting for a distant pullback
        let want_entry = a.verdict.entry_price;
        if want_entry > 0.0 && price > 0.0 && want_entry < price * (1.0 - MAX_ENTRY_DISTANCE_PCT) {
            let gap_pct = (price - want_entry) / price * 100.0;
            tracing::info!(
                "skipping pending buy plan for {}: entry {:.6} is {:.1}% below market {:.6} (exceeds max {:.0}%) — waiting for a closer setup",
                a.symbol,
                want_entry,
                gap_pct,
                price,
                MAX_ENTRY_DISTANCE_PCT * 100.0
            );
            return Ok(());
        }
        // A pending entry is still an executable intent, so require the exact same BUY +
        // consensus checks as an immediate order. This rejects HOLD verdicts that happen to
        // contain stale entry/target/stop fields.
        match self.validate_executable_long(a, s, price) {
            Ok(rr) => {
                let plan = self.build_plan(
                    account_id,
                    a,
                    PlanState::Pending,
                    Action::Buy,
                    decision_id,
                    price,
                    s,
                );
                let pid = self.plans.upsert(&plan).await?;
                tracing::info!(
                    "placed pending buy plan for {} at {} (target {} / stop {}, RR {:.2})",
                    a.symbol,
                    plan.entry_price,
                    plan.target_price,
                    plan.stop_price,
                    rr
                );
                self.events.publish(&LiveEvent::Decision {
                    record: DecisionRecord {
                        id: pid,
                        ..self.to_record(account_id, a, s.mode)
                    },
                    analysis: a.clone(),
                });
            }
            Err(reason) => tracing::info!("not placing pending buy plan for {}: {reason}", a.symbol),
        }
        Ok(())
    }

    fn plan_levels(&self, a: &Analysis, price: f64, s: &TradingSettings) -> (f64, f64, f64) {
        let v = &a.verdict;
        let entry = if v.entry_price > 0.0 {
            v.entry_price
        } else {
            price
        };
        let target = if v.target_price > 0.0 {
            v.target_price
        } else if s.take_profit_pct > 0.0 {
            entry * (1.0 + s.take_profit_pct)
        } else {
            0.0
        };
        let stop = if v.stop_price > 0.0 {
            v.stop_price
        } else if s.stop_loss_pct > 0.0 {
            entry * (1.0 - s.stop_loss_pct)
        } else {
            0.0
        };
        (entry, target, stop)
    }

    /// validate + return the "reason" it failed (also used to display in the Targets view)
    pub fn validate_trackable_plan(
        &self,
        a: &Analysis,
        s: &TradingSettings,
        price: f64,
    ) -> Result<f64, String> {
        if a.synthetic {
            return Err("price data is synthetic, skipping auto plan".into());
        }
        if a.consensus.vetoed {
            return Err("an agent vetoed".into());
        }
        if a.verdict.confidence < s.min_confidence {
            return Err(format!(
                "confidence {:.2} below threshold {:.2}",
                a.verdict.confidence, s.min_confidence
            ));
        }
        let (entry, target, stop) = self.plan_levels(a, price, s);
        validate_long_plan(entry, target, stop)
    }

    fn validate_executable_long(
        &self,
        a: &Analysis,
        s: &TradingSettings,
        price: f64,
    ) -> Result<f64, String> {
        if a.verdict.action != Action::Buy {
            return Err(format!(
                "must confirm as BUY only (got {})",
                a.verdict.action.as_str()
            ));
        }
        if !a.consensus.passed_threshold {
            return Err("consensus has not passed threshold for execute".into());
        }
        self.validate_trackable_plan(a, s, price)
    }

    #[allow(clippy::too_many_arguments)]
    fn build_plan(
        &self,
        account_id: i64,
        a: &Analysis,
        state: PlanState,
        action: Action,
        decision_id: i64,
        price: f64,
        s: &TradingSettings,
    ) -> TradePlan {
        let v = &a.verdict;
        let (entry, target, stop) = self.plan_levels(a, price, s);
        let entry_type = if matches!(state, PlanState::Open) {
            "market".into()
        } else if v.entry_type.trim().is_empty() || v.entry_type == "market" {
            "limit".into()
        } else {
            v.entry_type.clone()
        };
        TradePlan {
            id: 0,
            account_id,
            symbol: a.symbol.clone(),
            quote: a.quote.clone(),
            state,
            action,
            entry_type,
            entry_price: if matches!(state, PlanState::Open) {
                price
            } else {
                entry
            },
            target_price: target,
            stop_price: stop,
            confidence: v.confidence,
            thesis: v.thesis.clone(),
            invalidation: v.invalidation.clone(),
            next_step: v.next_step.clone(),
            decision_id: Some(decision_id),
            last_price: price,
            // seed active-management fields: remember the entry-time stop (defines risk R) and
            // start the high-water mark at the entry price; trailing kicks in later in check_exits
            high_water_mark: if matches!(state, PlanState::Open) { price } else { entry },
            initial_stop: stop,
            trail_active: false,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }
    }

    async fn enter_now(
        &self,
        account_id: i64,
        a: &Analysis,
        s: &TradingSettings,
        decision_id: i64,
        price: f64,
    ) {
        if s.paused {
            tracing::warn!("not entering {}: system is paused/kill-switch active", a.symbol);
            return;
        }
        if let Err(reason) = self.validate_executable_long(a, s, price) {
            tracing::info!("not entering {} immediately: {reason}", a.symbol);
            return;
        }
        let uses_live_broker = self.uses_live_broker(account_id, s).await;
        match self.current_position_result(account_id, &a.symbol, s).await {
            Ok(Some(pos)) if pos.amount_base > 0.0 => {
                if uses_live_broker {
                    self.ensure_live_rescue_plans(account_id, s, std::slice::from_ref(&pos))
                        .await;
                }
                tracing::warn!(
                    symbol = %a.symbol,
                    amount = pos.amount_base,
                    avg = pos.avg_price,
                    "not adding to position: Bitkub/Wallet already has an open position"
                );
                return;
            }
            Ok(_) => {}
            Err(e) if uses_live_broker => {
                tracing::warn!(
                    symbol = %a.symbol,
                    "not adding to position: failed to read Bitkub position before buying: {e}"
                );
                return;
            }
            Err(e) => {
                tracing::warn!(symbol = %a.symbol, "failed to read wallet position before buying: {e}");
                return;
            }
        }
        match self
            .execute(
                account_id,
                &a.symbol,
                &a.quote,
                Action::Buy,
                s.trade_amount_quote,
                s.mode,
                Some(decision_id),
            )
            .await
        {
            Ok(_) => {
                let plan = self.build_plan(
                    account_id,
                    a,
                    PlanState::Open,
                    Action::Buy,
                    decision_id,
                    price,
                    s,
                );
                self.plans.upsert(&plan).await.ok();
                tracing::info!(
                    "entering {} at market immediately (target {} / stop {})",
                    a.symbol,
                    plan.target_price,
                    plan.stop_price
                );
            }
            Err(e) => tracing::warn!("failed to enter {}: {e}", a.symbol),
        }
    }

    // ---------------- MONITOR: watch price (lightweight, no AI calls) ----------------

    /// Watch pending plans — if price reaches the planned entry level → act
    pub async fn check_triggers(&self, account_id: i64, settings: &TradingSettings) {
        if !settings.auto_trade
            || settings.paused
            || matches!(settings.mode, TradingMode::SignalOnly)
        {
            return;
        }
        let plans = match self.plans.active(account_id).await {
            Ok(p) => p,
            Err(_) => return,
        };
        for p in plans {
            if !matches!(p.state, PlanState::Pending) || p.action != Action::Buy {
                continue;
            }
            let plan_age = Utc::now().signed_duration_since(p.created_at);
            if plan_age > ChronoDuration::hours(MAX_PENDING_PLAN_AGE_HOURS) {
                tracing::info!(
                    "plan for {} is older than {} hours → re-analyzing",
                    p.symbol,
                    MAX_PENDING_PLAN_AGE_HOURS
                );
                self.refresh_pending_plan(account_id, &p, settings).await;
                continue;
            }
            let price = self
                .live_broker
                .last_price(&p.symbol, &p.quote)
                .await
                .unwrap_or(0.0);
            if price <= 0.0 {
                continue;
            }
            self.plans
                .set_last_price(account_id, &p.symbol, price)
                .await
                .ok();
            if p.stop_price > 0.0 && price <= p.stop_price {
                self.alert(
                    account_id,
                    "info",
                    "plan_cancelled",
                    format!(
                        "cancelled pending buy plan for {}: price {price:.6} broke stop {:.6} before entry — setup is invalidated",
                        p.symbol, p.stop_price
                    ),
                )
                .await;
                self.plans
                    .set_state(account_id, &p.symbol, PlanState::Cancelled)
                    .await
                    .ok();
                continue;
            }
            if p.target_price > 0.0 && price >= p.target_price {
                self.alert(
                    account_id,
                    "info",
                    "plan_cancelled",
                    format!(
                        "cancelled pending buy plan for {}: price {price:.6} reached target {:.6} before entry — missed the move, waiting for new setup",
                        p.symbol, p.target_price
                    ),
                )
                .await;
                self.plans
                    .set_state(account_id, &p.symbol, PlanState::Cancelled)
                    .await
                    .ok();
                continue;
            }
            if p.entry_price > 0.0 && price <= p.entry_price {
                if plan_age <= ChronoDuration::minutes(FRESH_PLAN_DIRECT_EXEC_MINUTES) {
                    tracing::info!(
                        "reached entry level for fresh plan {} at {} → executing validated plan",
                        p.symbol,
                        price
                    );
                    self.execute_pending_plan(account_id, &p, settings, price)
                        .await;
                } else {
                    tracing::info!(
                        "reached entry level for older plan {} at {} → confirming with analysis",
                        p.symbol,
                        price
                    );
                    self.confirm_and_enter(account_id, &p, settings, price)
                        .await;
                }
            }
        }
    }

    async fn execute_pending_plan(
        &self,
        account_id: i64,
        p: &TradePlan,
        settings: &TradingSettings,
        price: f64,
    ) {
        if p.confidence < settings.min_confidence {
            tracing::info!(
                "fresh plan for {} no longer meets account confidence: {:.2} < {:.2} → cancelling",
                p.symbol,
                p.confidence,
                settings.min_confidence
            );
            self.plans
                .set_state(account_id, &p.symbol, PlanState::Cancelled)
                .await
                .ok();
            return;
        }
        let rr = match validate_long_plan(price, p.target_price, p.stop_price) {
            Ok(rr) => rr,
            Err(reason) => {
                tracing::info!(
                    "fresh plan for {} is invalid at trigger price {}: {reason} → cancelling",
                    p.symbol,
                    price
                );
                self.plans
                    .set_state(account_id, &p.symbol, PlanState::Cancelled)
                    .await
                    .ok();
                return;
            }
        };
        match self
            .execute(
                account_id,
                &p.symbol,
                &p.quote,
                Action::Buy,
                settings.trade_amount_quote,
                settings.mode,
                p.decision_id,
            )
            .await
        {
            Ok(_) => {
                let now = Utc::now();
                let mut open = p.clone();
                open.state = PlanState::Open;
                open.entry_type = "market".into();
                open.entry_price = price;
                open.last_price = price;
                open.high_water_mark = price;
                open.initial_stop = p.stop_price;
                open.trail_active = false;
                open.created_at = now;
                open.updated_at = now;
                self.plans.upsert(&open).await.ok();
                tracing::info!(
                    "entered fresh validated plan for {} at {} (RR {:.2})",
                    p.symbol,
                    price,
                    rr
                );
            }
            Err(e) => {
                tracing::warn!(
                    "failed to execute fresh planned buy for {}: {e} → cancelling plan",
                    p.symbol
                );
                self.plans
                    .set_state(account_id, &p.symbol, PlanState::Cancelled)
                    .await
                    .ok();
            }
        }
    }

    async fn confirm_and_enter(
        &self,
        account_id: i64,
        p: &TradePlan,
        settings: &TradingSettings,
        price: f64,
    ) {
        let analysis = match self
            .analyze_for_account(account_id, &p.symbol, settings)
            .await
        {
            Ok(a) => a,
            Err(e) => {
                tracing::warn!("could not confirm plan for {}: {e} → cancelling", p.symbol);
                self.plans
                    .set_state(account_id, &p.symbol, PlanState::Cancelled)
                    .await
                    .ok();
                return;
            }
        };
        let rec = self.to_record(account_id, &analysis, settings.mode);
        let did = self.repo.save_decision(&rec).await.ok();
        if let Some(id) = did {
            self.repo
                .save_analysis_json(id, &serde_json::to_value(&analysis).unwrap_or(json!({})))
                .await
                .ok();
            self.events.publish(&LiveEvent::Decision {
                record: DecisionRecord { id, ..rec },
                analysis: analysis.clone(),
            });
        }
        match self.validate_executable_long(&analysis, settings, price) {
            Ok(rr) => {
                match self
                    .execute(
                        account_id,
                        &p.symbol,
                        &p.quote,
                        Action::Buy,
                        settings.trade_amount_quote,
                        settings.mode,
                        did,
                    )
                    .await
                {
                    Ok(_) => {
                        let plan = self.build_plan(
                            account_id,
                            &analysis,
                            PlanState::Open,
                            Action::Buy,
                            did.unwrap_or(p.decision_id.unwrap_or(0)),
                            price,
                            settings,
                        );
                        self.plans.upsert(&plan).await.ok();
                        tracing::info!(
                            "confirmed, entering buy for {} at {} (RR {:.2})",
                            p.symbol,
                            price,
                            rr
                        );
                    }
                    Err(e) => {
                        // Failed to place order (insufficient funds / broker rejected) — cancel plan to prevent loop
                        // re-analyzing every tick even though the buy cannot go through (execute already sent alert)
                        tracing::warn!("failed to execute planned buy for {}: {e} → cancelling plan", p.symbol);
                        self.plans
                            .set_state(account_id, &p.symbol, PlanState::Cancelled)
                            .await
                            .ok();
                    }
                }
            }
            Err(reason) => {
                tracing::info!("reached level but not confirmed → cancelling plan for {}: {reason}", p.symbol);
                self.plans
                    .set_state(account_id, &p.symbol, PlanState::Cancelled)
                    .await
                    .ok();
            }
        }
    }

    async fn refresh_pending_plan(
        &self,
        account_id: i64,
        p: &TradePlan,
        settings: &TradingSettings,
    ) {
        let analysis = match self
            .analyze_for_account(account_id, &p.symbol, settings)
            .await
        {
            Ok(a) => a,
            Err(e) => {
                tracing::warn!("could not refresh plan for {}: {e} → cancelling", p.symbol);
                self.plans
                    .set_state(account_id, &p.symbol, PlanState::Cancelled)
                    .await
                    .ok();
                return;
            }
        };
        let rec = self.to_record(account_id, &analysis, settings.mode);
        let did = self.repo.save_decision(&rec).await.ok();
        if let Some(id) = did {
            self.repo
                .save_analysis_json(id, &serde_json::to_value(&analysis).unwrap_or(json!({})))
                .await
                .ok();
            self.events.publish(&LiveEvent::Decision {
                record: DecisionRecord { id, ..rec },
                analysis: analysis.clone(),
            });
            if let Err(e) = self
                .plan_from_analysis(account_id, &analysis, settings, id)
                .await
            {
                tracing::warn!("refresh plan for {} failed: {e}", p.symbol);
            }
        } else {
            self.plans
                .set_state(account_id, &p.symbol, PlanState::Cancelled)
                .await
                .ok();
        }
    }

    /// Symbols with active plans (used so the watcher skips redundant deep analysis)
    pub async fn active_plan_symbols(&self, account_id: i64) -> std::collections::HashSet<String> {
        self.plans
            .active(account_id)
            .await
            .unwrap_or_default()
            .into_iter()
            .map(|p| p.symbol)
            .collect()
    }

    /// Symbols of currently-held positions (plans in the `open` state). These should be
    /// re-analyzed every deep tick so the AI keeps managing them (vs buy-and-forget).
    pub async fn held_plan_symbols(&self, account_id: i64) -> std::collections::HashSet<String> {
        self.plans
            .active(account_id)
            .await
            .unwrap_or_default()
            .into_iter()
            .filter(|p| matches!(p.state, PlanState::Open))
            .map(|p| p.symbol)
            .collect()
    }

    pub async fn has_active_plan(&self, account_id: i64, symbol: &str) -> bool {
        self.plans
            .get(account_id, symbol)
            .await
            .ok()
            .flatten()
            .map(|p| matches!(p.state, PlanState::Pending | PlanState::Open))
            .unwrap_or(false)
    }

    /// Watch open positions — manage the trailing stop, then close when the (managed) stop or target
    /// is reached. Returns the symbols that were closed this pass (so the caller can apply a
    /// re-entry cooldown and avoid immediately re-buying a coin it just exited).
    pub async fn check_exits(&self, account_id: i64, settings: &TradingSettings) -> Vec<String> {
        let mut exited: Vec<String> = Vec::new();
        if !settings.auto_trade
            || settings.paused
            || !settings.allow_sell
            || matches!(settings.mode, TradingMode::SignalOnly)
        {
            return exited;
        }
        let live_positions = if matches!(settings.mode, TradingMode::Live) {
            match self.reconcile_live_account(account_id, settings).await {
                Ok((_, positions)) => positions,
                Err(e) => {
                    tracing::warn!("failed to sync live position before checking exits: {e}");
                    Vec::new()
                }
            }
        } else {
            Vec::new()
        };
        let cfg = ManageCfg::from_style(&settings.manage_style);
        let plans = self.plans.active(account_id).await.unwrap_or_default();
        for p in plans
            .into_iter()
            .filter(|p| matches!(p.state, PlanState::Open))
        {
            let pos = if matches!(settings.mode, TradingMode::Live) {
                live_positions
                    .iter()
                    .find(|pos| pos.symbol.eq_ignore_ascii_case(&p.symbol))
                    .cloned()
            } else {
                self.wallet
                    .position(account_id, &p.symbol)
                    .await
                    .ok()
                    .flatten()
            };
            let pos = match pos {
                Some(pos) if pos.amount_base > 0.0 => pos,
                _ => {
                    self.plans
                        .set_state(account_id, &p.symbol, PlanState::Closed)
                        .await
                        .ok();
                    continue;
                }
            };
            let price = self
                .live_broker
                .last_price(&p.symbol, &p.quote)
                .await
                .unwrap_or(0.0);
            if price <= 0.0 {
                continue;
            }
            self.plans
                .set_last_price(account_id, &p.symbol, price)
                .await
                .ok();

            let target = if p.target_price > 0.0 {
                p.target_price
            } else if settings.take_profit_pct > 0.0 {
                pos.avg_price * (1.0 + settings.take_profit_pct)
            } else {
                0.0
            };
            // current stop, falling back to the settings stop-loss% if the plan never set one
            let settings_stop = if settings.stop_loss_pct > 0.0 {
                pos.avg_price * (1.0 - settings.stop_loss_pct)
            } else {
                0.0
            };
            let current_stop = if p.stop_price > 0.0 {
                p.stop_price
            } else {
                settings_stop
            };
            let initial_stop = if p.initial_stop > 0.0 {
                p.initial_stop
            } else {
                current_stop
            };

            // ----- active management: ratchet the trailing stop / breakeven -----
            let high_water = p.high_water_mark.max(price);
            let target_reached = target > 0.0 && price >= target;
            // "let winners run": once the target is reached, tighten the trail instead of hard-selling
            let eff_cfg = if cfg.enabled && settings.let_winners_run && target_reached {
                ManageCfg {
                    trail_r: cfg.trail_r * 0.5,
                    ..cfg
                }
            } else {
                cfg
            };
            let (new_stop, active) = managed_stop(
                pos.avg_price,
                price,
                high_water,
                initial_stop,
                current_stop,
                eff_cfg,
            );
            if cfg.enabled && (price > p.high_water_mark + 1e-12 || new_stop > p.stop_price + 1e-9) {
                self.plans
                    .update_trailing(account_id, &p.symbol, high_water, new_stop, active)
                    .await
                    .ok();
                if new_stop > p.stop_price + 1e-9 {
                    let note = if new_stop >= pos.avg_price {
                        "now risk-free, trailing profit"
                    } else {
                        "reducing risk"
                    };
                    self.alert(
                        account_id,
                        "info",
                        "position_managed",
                        format!(
                            "🛡️ {}: stop raised to {new_stop:.6} (from {:.6}) — {note}",
                            p.symbol, p.stop_price
                        ),
                    )
                    .await;
                }
            }

            // ----- exit decision -----
            // Catastrophic cap: floor the effective stop at avg*(1-MAX_LOSS_PCT) so no single trade
            // ever loses more than MAX_LOSS_PCT — even if the plan's own stop is wider or missing.
            let plan_stop = if cfg.enabled { new_stop } else { current_stop };
            let catastrophic = pos.avg_price * (1.0 - MAX_LOSS_PCT);
            let eff_stop = plan_stop.max(catastrophic);
            // Exits are driven ONLY by: the hard stop / catastrophic cap (a definite loss), the
            // trailing stop / profit-lock (protect booked gains), and take-profit. There is no
            // time-based cut — a position may stay underwater as long as it wants as long as it
            // holds above the stop; the AI's thesis-reversal exit (in plan_from_analysis) handles
            // "the prediction was wrong → get out". This matches "long drawdowns that recover are fine".
            let reason = if eff_stop > 0.0 && price <= eff_stop {
                if plan_stop < catastrophic - 1e-9 {
                    // the plan stop was wider than the hard cap → this is the catastrophic cap firing
                    Some("max-loss-cap")
                } else if cfg.enabled && new_stop > initial_stop + 1e-9 {
                    Some("trailing-stop")
                } else {
                    Some("stop-loss")
                }
            } else if target_reached {
                // hard take-profit only when not letting winners run (or management disabled)
                if cfg.enabled && settings.let_winners_run {
                    None
                } else {
                    Some("take-profit")
                }
            } else {
                None
            };

            if let Some(r) = reason {
                let pnl_pct = if pos.avg_price > 0.0 {
                    (price / pos.avg_price - 1.0) * 100.0
                } else {
                    0.0
                };
                self.alert(
                    account_id,
                    "info",
                    "position_exit",
                    format!(
                        "closing position {} ({r}) at {price:.6} — avg cost {:.6} ({pnl_pct:+.2}%)",
                        p.symbol, pos.avg_price
                    ),
                )
                .await;
                self.exit_position(account_id, &p.symbol, r, p.decision_id)
                    .await;
                self.plans
                    .set_state(account_id, &p.symbol, PlanState::Closed)
                    .await
                    .ok();
                exited.push(p.symbol.clone());
            }
        }
        exited
    }

    async fn exit_position(
        &self,
        account_id: i64,
        symbol: &str,
        _reason: &str,
        decision_id: Option<i64>,
    ) {
        let settings = self
            .settings
            .get(account_id)
            .await
            .unwrap_or_else(|_| default_settings());
        let pos = match settings.mode {
            TradingMode::Live => match self.live_positions_from_broker(account_id, QUOTE).await {
                Ok((_, positions)) => positions
                    .into_iter()
                    .find(|p| p.symbol.eq_ignore_ascii_case(symbol)),
                Err(_) => None,
            },
            TradingMode::Paper | TradingMode::SignalOnly => self
                .wallet
                .position(account_id, symbol)
                .await
                .ok()
                .flatten(),
        };
        let pos = match pos {
            Some(p) if p.amount_base > 0.0 => p,
            _ => return,
        };
        let price = self
            .live_broker
            .last_price(symbol, QUOTE)
            .await
            .unwrap_or(pos.avg_price);
        let amount_quote = pos.amount_base * price;
        if let Err(e) = self
            .execute(
                account_id,
                symbol,
                QUOTE,
                Action::Sell,
                amount_quote,
                settings.mode,
                decision_id,
            )
            .await
        {
            tracing::warn!("failed to close position for {symbol}: {e}");
        }
    }

    // ---------------- order execution (shared across all call sites) ----------------

    async fn is_live_account(&self, account_id: i64) -> bool {
        matches!(
            self.accounts.get(account_id).await.ok().flatten(),
            Some(acc) if matches!(acc.kind, AccountKind::Live)
        )
    }

    fn broker_order_to_trade(account_id: i64, o: BrokerOrder) -> TradeRecord {
        TradeRecord {
            id: 0,
            account_id,
            decision_id: None,
            symbol: o.symbol,
            quote: o.quote,
            side: o.side,
            mode: TradingMode::Live,
            simulated: false,
            amount_base: o.amount_base,
            amount_quote: o.amount_quote,
            price: o.price,
            status: "filled".into(),
            external_order_id: o.order_id,
            note: "bitkub history sync".into(),
            realized_pnl: 0.0,
            created_at: o.created_at,
        }
    }

    async fn sync_symbol_history(
        &self,
        account_id: i64,
        broker: &Arc<dyn Broker>,
        symbol: &str,
        quote: &str,
    ) -> DomainResult<Vec<BrokerOrder>> {
        // Fetch deep history (walk all pages in adapter) to reconstruct average cost fully across all trades
        match broker.order_history(symbol, quote, 5000).await {
            Ok(orders) => {
                let count = orders.len();
                for o in orders.iter().cloned() {
                    let tr = Self::broker_order_to_trade(account_id, o);
                    if let Err(e) = self.trades.save_external(&tr).await {
                        tracing::warn!(
                            symbol,
                            external_order_id = %tr.external_order_id,
                            "failed to sync Bitkub trade history: {e}"
                        );
                    }
                }
                tracing::info!(symbol, orders = count, "Bitkub my-order-history sync succeeded");
                Ok(orders)
            }
            Err(e) => {
                tracing::warn!(symbol, "failed to read Bitkub my-order-history: {e}");
                Err(e)
            }
        }
    }

    fn avg_price_from_orders(orders: &[BrokerOrder], current_base: f64) -> Option<f64> {
        let mut rows = orders.to_vec();
        rows.sort_by_key(|o| o.created_at);
        let mut base = 0.0;
        let mut cost = 0.0;
        for o in rows {
            if o.amount_base <= 0.0 || o.price <= 0.0 {
                continue;
            }
            match o.side {
                Action::Buy => {
                    base += o.amount_base;
                    cost += if o.amount_quote > 0.0 {
                        o.amount_quote
                    } else {
                        o.amount_base * o.price
                    };
                }
                Action::Sell => {
                    if base > 0.0 {
                        let sold = o.amount_base.min(base);
                        let avg = cost / base;
                        base -= sold;
                        cost -= avg * sold;
                    }
                }
                Action::Hold => {}
            }
        }
        if base > 0.0 && cost > 0.0 {
            // Bitkub balance is the source of truth for currently held amount; use the order-derived
            // average cost even if fee rounding makes base slightly different.
            let _ = current_base;
            Some(cost / base)
        } else {
            None
        }
    }

    async fn avg_price_from_trades(
        &self,
        account_id: i64,
        symbol: &str,
        current_base: f64,
        fallback_price: f64,
    ) -> f64 {
        let mut rows = self
            .trades
            .for_symbol(account_id, symbol, 200)
            .await
            .unwrap_or_default();
        rows.sort_by_key(|t| t.created_at);

        let mut base = 0.0;
        let mut cost = 0.0;
        for t in rows
            .into_iter()
            .filter(|t| t.status == "filled" && matches!(t.mode, TradingMode::Live))
        {
            if t.amount_base <= 0.0 || t.price <= 0.0 {
                continue;
            }
            match t.side {
                Action::Buy => {
                    base += t.amount_base;
                    cost += if t.amount_quote > 0.0 {
                        t.amount_quote
                    } else {
                        t.amount_base * t.price
                    };
                }
                Action::Sell => {
                    if base > 0.0 {
                        let sold = t.amount_base.min(base);
                        let avg = cost / base;
                        base -= sold;
                        cost -= avg * sold;
                    }
                }
                Action::Hold => {}
            }
        }

        if base > 0.0 && cost > 0.0 {
            cost / base
        } else if current_base > 0.0 {
            fallback_price
        } else {
            0.0
        }
    }

    pub async fn live_positions_from_broker(
        &self,
        account_id: i64,
        quote: &str,
    ) -> DomainResult<(f64, Vec<PaperPosition>)> {
        let broker = self.broker_resolver.resolve(account_id).await?;
        let balances = broker.balances().await?;
        let mut cash = 0.0;
        let mut positions = Vec::new();

        for b in balances {
            let asset = b.asset.to_uppercase();
            if asset == quote.to_uppercase() {
                cash = b.available;
                continue;
            }
            if b.available <= 0.0 {
                continue;
            }
            let last = match broker.last_price(&asset, quote).await {
                Ok(p) => p,
                Err(_) => self
                    .live_broker
                    .last_price(&asset, quote)
                    .await
                    .unwrap_or(0.0),
            };
            if last <= 0.0 || b.available * last < 1.0 {
                continue;
            }
            let orders = self
                .sync_symbol_history(account_id, &broker, &asset, quote)
                .await
                .unwrap_or_default();
            let avg_price = Self::avg_price_from_orders(&orders, b.available).unwrap_or(
                self.avg_price_from_trades(account_id, &asset, b.available, last)
                    .await,
            );
            positions.push(PaperPosition {
                symbol: asset,
                amount_base: b.available,
                avg_price,
                last_price: last,
            });
        }
        positions.sort_by(|a, b| a.symbol.cmp(&b.symbol));
        Ok((cash, positions))
    }

    /// Fetch open orders (unmatched limit orders) for the live account for tracked symbols
    /// — Bitkub my-open-orders requires a symbol, so only pairs being watched/planned/held are visible
    pub async fn live_open_orders(&self, account_id: i64, symbols: &[String]) -> Vec<OpenOrder> {
        let broker = match self.broker_resolver.resolve(account_id).await {
            Ok(b) => b,
            Err(e) => {
                tracing::warn!("failed to resolve broker to read open orders: {e}");
                return Vec::new();
            }
        };
        let mut out = Vec::new();
        for sym in symbols {
            match broker.open_orders(sym, QUOTE).await {
                Ok(mut v) => out.append(&mut v),
                Err(e) => tracing::warn!(symbol = %sym, "failed to read open orders: {e}"),
            }
        }
        out.sort_by(|a, b| a.symbol.cmp(&b.symbol));
        out
    }

    fn rescue_exit_levels(avg_price: f64, settings: &TradingSettings) -> (f64, f64) {
        let take = if settings.take_profit_pct > 0.0 {
            settings.take_profit_pct
        } else {
            RESCUE_TAKE_PROFIT_PCT
        };
        let stop = if settings.stop_loss_pct > 0.0 {
            settings.stop_loss_pct
        } else {
            RESCUE_STOP_LOSS_PCT
        };
        (avg_price * (1.0 + take), avg_price * (1.0 - stop))
    }

    pub async fn ensure_live_rescue_plans(
        &self,
        account_id: i64,
        settings: &TradingSettings,
        positions: &[PaperPosition],
    ) {
        for pos in positions {
            if pos.amount_base <= 0.0 || pos.avg_price <= 0.0 {
                continue;
            }
            let has_plan = self
                .plans
                .get(account_id, &pos.symbol)
                .await
                .ok()
                .flatten()
                .map(|p| matches!(p.state, PlanState::Pending | PlanState::Open))
                .unwrap_or(false);
            if has_plan {
                continue;
            }
            let (target, stop) = Self::rescue_exit_levels(pos.avg_price, settings);
            let plan = TradePlan {
                id: 0,
                account_id,
                symbol: pos.symbol.clone(),
                quote: QUOTE.into(),
                state: PlanState::Open,
                action: Action::Buy,
                entry_type: "broker-sync".into(),
                entry_price: pos.avg_price,
                target_price: target,
                stop_price: stop,
                confidence: 0.50,
                thesis: "created from Bitkub balance/order history because coins are held in real account but no plan exists in the system".into(),
                invalidation: format!(
                    "close position if price breaks stop {:.6} or AI analysis reverses the thesis",
                    stop
                ),
                next_step: format!(
                    "watch target {:.6} / stop {:.6}; Bitkub is the source of truth",
                    target, stop
                ),
                decision_id: None,
                last_price: pos.last_price,
                high_water_mark: pos.last_price.max(pos.avg_price),
                initial_stop: stop,
                trail_active: false,
                created_at: Utc::now(),
                updated_at: Utc::now(),
            };
            if let Err(e) = self.plans.upsert(&plan).await {
                tracing::warn!(symbol = %pos.symbol, "failed to create rescue plan: {e}");
            } else {
                self.alert(
                    account_id,
                    "warn",
                    "rescue_plan",
                    format!(
                        "found {} in live account but no plan in system — created rescue plan (target {target:.6} / stop {stop:.6})",
                        pos.symbol
                    ),
                )
                .await;
            }
        }
    }

    /// Heal trades recorded with amount=0 (e.g. when broker returned incomplete fill) — fetch history from Bitkub
    /// and backfill via save_external (ON CONFLICT) even if the coins are no longer held.
    /// `held` = coins already synced in this reconcile pass, to avoid duplication
    pub async fn heal_zero_amount_trades(&self, account_id: i64, held: &[String]) {
        let symbols = match self.trades.symbols_with_zero_amount(account_id).await {
            Ok(s) => s,
            Err(e) => {
                tracing::warn!("failed to find symbols with amount=0: {e}");
                return;
            }
        };
        if symbols.is_empty() {
            return;
        }
        let broker = match self.broker_resolver.resolve(account_id).await {
            Ok(b) => b,
            Err(e) => {
                tracing::warn!("failed to resolve broker to heal amount=0 trades: {e}");
                return;
            }
        };
        for sym in symbols {
            if held.iter().any(|h| h.eq_ignore_ascii_case(&sym)) {
                continue; // already synced in this reconcile pass
            }
            match self.sync_symbol_history(account_id, &broker, &sym, QUOTE).await {
                Ok(_) => tracing::info!(symbol = %sym, "backfilled history to repair previously zero-amount trades"),
                Err(e) => tracing::warn!(symbol = %sym, "failed to heal amount=0 trades: {e}"),
            }
        }
    }

    /// Recompute realized P&L retroactively for all coins in the account — lets the Dashboard count wins/losses/closed positions
    /// from history synced from the broker (idempotent: only writes rows whose values changed)
    pub async fn backfill_realized(&self, account_id: i64) {
        let symbols = self.trades.distinct_symbols(account_id).await.unwrap_or_default();
        for sym in symbols {
            match self.trades.recompute_realized(account_id, &sym).await {
                Ok(n) if n > 0 => {
                    tracing::info!(symbol = %sym, updated = n, "backfill realized P&L complete")
                }
                Ok(_) => {}
                Err(e) => tracing::warn!(symbol = %sym, "backfill realized P&L failed: {e}"),
            }
        }
    }

    pub async fn reconcile_live_account(
        &self,
        account_id: i64,
        settings: &TradingSettings,
    ) -> DomainResult<(f64, Vec<PaperPosition>)> {
        let (cash, positions) = self.live_positions_from_broker(account_id, QUOTE).await?;
        self.ensure_live_rescue_plans(account_id, settings, &positions)
            .await;
        let held: Vec<String> = positions.iter().map(|p| p.symbol.clone()).collect();
        self.heal_zero_amount_trades(account_id, &held).await;
        // After history sync is complete → backfill realized P&L so Dashboard figures are accurate
        self.backfill_realized(account_id).await;
        Ok((cash, positions))
    }

    pub async fn wallet_view(&self, account_id: i64) -> DomainResult<WalletView> {
        if self.is_live_account(account_id).await {
            let settings = self
                .settings
                .get(account_id)
                .await
                .unwrap_or_else(|_| default_settings());
            let (cash, positions) = self.reconcile_live_account(account_id, &settings).await?;
            let positions_value: f64 = positions.iter().map(|p| p.amount_base * p.last_price).sum();
            let cost: f64 = positions.iter().map(|p| p.amount_base * p.avg_price).sum();
            let equity = cash + positions_value;
            let basis = cash + cost;
            let pnl = positions_value - cost;
            return Ok(WalletView {
                cash_quote: cash,
                starting_cash: basis,
                positions,
                positions_value,
                equity,
                pnl,
                pnl_pct: if basis > 0.0 { pnl / basis } else { 0.0 },
                simulated: false,
            });
        }

        let (cash, starting, mut positions) = self.wallet.view(account_id).await?;
        let mut positions_value = 0.0;
        for p in positions.iter_mut() {
            let last = self
                .live_broker
                .last_price(&p.symbol, QUOTE)
                .await
                .unwrap_or(p.avg_price);
            p.last_price = last;
            positions_value += p.amount_base * last;
        }
        let equity = cash + positions_value;
        let pnl = equity - starting;
        Ok(WalletView {
            cash_quote: cash,
            starting_cash: starting,
            positions,
            positions_value,
            equity,
            pnl,
            pnl_pct: if starting > 0.0 { pnl / starting } else { 0.0 },
            simulated: true,
        })
    }

    fn risk_config(settings: &TradingSettings) -> RiskConfig {
        RiskConfig {
            max_position_pct: settings.max_position_pct,
            daily_loss_limit: settings.daily_loss_limit,
            max_open_positions: settings.max_open_positions.max(0) as usize,
        }
    }

    /// Portfolio overview for the account (used for risk calculation + governor)
    pub async fn portfolio_snapshot(
        &self,
        account_id: i64,
        mode: TradingMode,
        quote: &str,
    ) -> PortfolioSnapshot {
        match mode {
            TradingMode::Paper | TradingMode::SignalOnly => {
                let (cash, starting, positions) =
                    self.wallet
                        .view(account_id)
                        .await
                        .unwrap_or((0.0, 0.0, vec![]));
                let mut positions_value = 0.0;
                for p in positions.iter() {
                    let price = self
                        .live_broker
                        .last_price(&p.symbol, quote)
                        .await
                        .unwrap_or(p.avg_price);
                    positions_value += p.amount_base * price;
                }
                let equity = cash + positions_value;
                let session_pnl_pct = if starting > 0.0 {
                    (equity - starting) / starting
                } else {
                    0.0
                };
                let deployed_pct = if equity > 0.0 {
                    positions_value / equity
                } else {
                    0.0
                };
                PortfolioSnapshot {
                    equity,
                    daily_pnl_pct: session_pnl_pct,
                    open_positions: positions.len(),
                    cash_thb: cash,
                    deployed_pct,
                    session_pnl_pct,
                }
            }
            TradingMode::Live => {
                let settings = self
                    .settings
                    .get(account_id)
                    .await
                    .unwrap_or_else(|_| default_settings());
                let (cash, positions) = self
                    .reconcile_live_account(account_id, &settings)
                    .await
                    .unwrap_or((0.0, vec![]));
                let positions_value: f64 =
                    positions.iter().map(|p| p.amount_base * p.last_price).sum();
                let unrealized: f64 = positions
                    .iter()
                    .map(|p| (p.last_price - p.avg_price) * p.amount_base)
                    .sum();
                let equity = cash + positions_value;
                let since = self
                    .wallet
                    .session_start(account_id)
                    .await
                    .unwrap_or_else(|_| chrono::DateTime::<Utc>::from_timestamp(0, 0).unwrap());
                let realized = self
                    .trades
                    .realized_since(account_id, since)
                    .await
                    .unwrap_or(0.0);
                let session_pnl_pct = if equity > 0.0 {
                    (realized + unrealized) / equity
                } else {
                    0.0
                };
                let deployed_pct = if equity > 0.0 {
                    positions_value / equity
                } else {
                    0.0
                };
                PortfolioSnapshot {
                    equity,
                    daily_pnl_pct: session_pnl_pct,
                    open_positions: positions.len(),
                    cash_thb: cash,
                    deployed_pct,
                    session_pnl_pct,
                }
            }
        }
    }

    /// Governor state for the account (funds + risk + current activity)
    pub async fn governor_state(&self, account_id: i64) -> crate::domain::models::GovernorState {
        let settings = self
            .settings
            .get(account_id)
            .await
            .unwrap_or_else(|_| default_settings());
        // Mode for "balance display": based on account kind, not trading mode
        //   - Live account → always fetch real balance from Bitkub (even if mode=signal-only)
        //   - Paper account → simulated wallet
        // (status label still follows settings.mode as normal, e.g. signal-only shows "📡 Signal Mode")
        let is_live_account = matches!(
            self.accounts.get(account_id).await.ok().flatten(),
            Some(acc) if matches!(acc.kind, AccountKind::Live)
        );
        let display_mode = if is_live_account {
            TradingMode::Live
        } else {
            TradingMode::Paper
        };
        let snap = self
            .portfolio_snapshot(account_id, display_mode, QUOTE)
            .await;
        let cash = match display_mode {
            TradingMode::Paper | TradingMode::SignalOnly => self
                .wallet
                .view(account_id)
                .await
                .map(|(c, _, _)| c)
                .unwrap_or(0.0),
            TradingMode::Live => match self.broker_resolver.resolve(account_id).await {
                Ok(b) => b.balance(QUOTE).await.map(|b| b.available).unwrap_or(0.0),
                Err(_) => 0.0,
            },
        };
        super::governor::evaluate(account_id, &settings, &snap, cash)
    }

    async fn risk_adjusted_buy_amount(
        &self,
        account_id: i64,
        settings: &TradingSettings,
        mode: TradingMode,
        quote: &str,
        requested_quote: f64,
    ) -> DomainResult<f64> {
        let cfg = Self::risk_config(settings);
        let mut pf = self.portfolio_snapshot(account_id, mode, quote).await;
        // A transient broker balance read can momentarily report 0 cash. Retry the snapshot once
        // before blocking so a network hiccup does not cancel an otherwise-valid buy (previously
        // surfaced as the misleading "risk cap reduced order size to 0").
        if pf.cash_thb <= 0.0 && matches!(mode, TradingMode::Live) {
            pf = self.portfolio_snapshot(account_id, mode, quote).await;
        }
        match risk::evaluate(&cfg, &pf, requested_quote) {
            RiskDecision::Allow { max_amount_quote } if max_amount_quote > 0.0 => {
                Ok(max_amount_quote)
            }
            RiskDecision::Allow { .. } => Err(DomainError::RiskBlocked(format!(
                "no available {quote} cash to open a position (balance read {:.2}) — skipping buy",
                pf.cash_thb
            ))),
            RiskDecision::Block { reason } => Err(DomainError::RiskBlocked(reason)),
        }
    }

    /// Available cash (quote) right now — paper: simulated wallet, live: balance from broker
    /// If unreadable → return 0 (let preflight block rather than guess and fail)
    async fn available_cash(&self, account_id: i64, mode: TradingMode, quote: &str) -> f64 {
        match mode {
            TradingMode::Paper | TradingMode::SignalOnly => self
                .wallet
                .view(account_id)
                .await
                .map(|(c, _, _)| c)
                .unwrap_or(0.0),
            TradingMode::Live => match self.broker_resolver.resolve(account_id).await {
                Ok(b) => {
                    let bal = b.balance(quote).await.map(|b| b.available).unwrap_or(0.0);
                    // retry once on a 0/failed read — guards against a transient balance hiccup
                    // blocking the order at preflight
                    if bal > 0.0 {
                        bal
                    } else {
                        b.balance(quote).await.map(|b| b.available).unwrap_or(0.0)
                    }
                }
                Err(e) => {
                    tracing::warn!("cannot read balance before buying (resolve broker): {e}");
                    0.0
                }
            },
        }
    }

    /// Actual amount of coins currently held (used to check before selling) — unreadable → 0
    async fn current_position_amount(
        &self,
        account_id: i64,
        symbol: &str,
        mode: TradingMode,
    ) -> f64 {
        match mode {
            TradingMode::Paper | TradingMode::SignalOnly => self
                .wallet
                .position(account_id, symbol)
                .await
                .ok()
                .flatten()
                .map(|p| p.amount_base)
                .unwrap_or(0.0),
            TradingMode::Live => match self.broker_resolver.resolve(account_id).await {
                Ok(b) => b
                    .balance(symbol)
                    .await
                    .map(|b| b.available)
                    .unwrap_or(0.0),
                Err(_) => 0.0,
            },
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn execute(
        &self,
        account_id: i64,
        symbol: &str,
        quote: &str,
        side: Action,
        amount_quote: f64,
        mode: TradingMode,
        decision_id: Option<i64>,
    ) -> DomainResult<TradeRecord> {
        if side == Action::Hold {
            return Err(DomainError::Broker("HOLD does not require a trade".into()));
        }
        let mut amount_quote = amount_quote;
        if side == Action::Buy {
            let settings = self
                .settings
                .get(account_id)
                .await
                .unwrap_or_else(|_| default_settings());
            amount_quote = match self
                .risk_adjusted_buy_amount(account_id, &settings, mode, quote, amount_quote)
                .await
            {
                Ok(a) => a,
                Err(e) => {
                    self.alert(
                        account_id,
                        "warn",
                        "risk_blocked",
                        format!("not buying {symbol}: {e}"),
                    )
                    .await;
                    return Err(e);
                }
            };
        }
        let price = self
            .live_broker
            .last_price(symbol, quote)
            .await
            .unwrap_or(0.0);
        // average cost of the position being sold (live only) — used to record realized P&L at sell time
        let mut live_sell_basis = 0.0_f64;

        // ---- preflight: reject orders that will definitely fail before sending to broker ----
        if side == Action::Buy {
            let cash = self.available_cash(account_id, mode, quote).await;
            match preflight::check_buy(amount_quote, cash, 0.0, price) {
                BuyPreflight::Proceed { amount_quote: a } => amount_quote = a,
                BuyPreflight::Shrink {
                    amount_quote: a,
                    reason,
                } => {
                    self.alert(
                        account_id,
                        "info",
                        "order_shrunk",
                        format!("{symbol}: {reason}"),
                    )
                    .await;
                    amount_quote = a;
                }
                BuyPreflight::Block { code, reason } => {
                    self.alert(account_id, "warn", code, format!("not buying {symbol}: {reason}"))
                        .await;
                    return Err(DomainError::Broker(reason));
                }
            }
        } else if side == Action::Sell {
            let held = self
                .current_position_amount(account_id, symbol, mode)
                .await;
            if let Err((code, reason)) = preflight::check_sell(held, price) {
                self.alert(account_id, "warn", code, format!("not selling {symbol}: {reason}"))
                    .await;
                return Err(DomainError::Broker(reason));
            }
            // Capture the average cost BEFORE selling so we can record realized P&L at sell time —
            // the broker does not report per-order P&L. Prefer the LOCAL ledger's average cost
            // (deterministic; works even when Bitkub returns avg=0); fall back to the broker's
            // reported avg only when we have no recorded buy legs. This guarantees the Dashboard
            // shows correct wins/losses immediately after every close.
            if matches!(mode, TradingMode::Live) {
                if let Ok((held, avg)) = self.trades.position_basis(account_id, symbol).await {
                    if held > 0.0 && avg > 0.0 {
                        live_sell_basis = avg;
                    }
                }
                if live_sell_basis <= 0.0 {
                    if let Ok((_, positions)) = self.live_positions_from_broker(account_id, quote).await {
                        live_sell_basis = positions
                            .iter()
                            .find(|p| p.symbol.eq_ignore_ascii_case(symbol))
                            .map(|p| p.avg_price)
                            .unwrap_or(0.0);
                    }
                }
            }
        }

        let (filled_base, realized, fill_price, simulated, ext_id, status, note) = match mode {
            TradingMode::Paper => match self
                .wallet
                .apply_fill(account_id, symbol, side == Action::Buy, amount_quote, price)
                .await
            {
                Ok(f) => (
                    f.filled_base,
                    f.realized_pnl,
                    price,
                    true,
                    String::new(),
                    "filled".to_string(),
                    "paper fill".to_string(),
                ),
                Err(e) => (
                    0.0,
                    0.0,
                    price,
                    true,
                    String::new(),
                    "failed".to_string(),
                    e.to_string(),
                ),
            },
            TradingMode::Live => {
                let req = OrderRequest {
                    symbol: symbol.to_string(),
                    quote: quote.to_string(),
                    action: side,
                    amount_quote,
                };
                let broker = self.broker_resolver.resolve(account_id).await?;
                match broker.place_order(&req).await {
                    Ok(r) => (
                        r.filled_amount,
                        0.0,
                        if r.price > 0.0 { r.price } else { price },
                        false,
                        r.order_id,
                        "filled".to_string(),
                        "bitkub order".to_string(),
                    ),
                    Err(e) => (
                        0.0,
                        0.0,
                        price,
                        false,
                        String::new(),
                        "failed".to_string(),
                        e.to_string(),
                    ),
                }
            }
            TradingMode::SignalOnly => {
                return Err(DomainError::Broker("signal-only mode does not trade".into()))
            }
        };

        // Guard against amount=0: order filled but broker returned incomplete quantity (e.g. last_price failed during fill calc)
        // → derive amount_base from "amount spent ÷ price" immediately so 0 is not written to DB (reconcile will correct it later)
        let filled_base = if status == "filled" && filled_base <= 0.0 && fill_price > 0.0 && amount_quote > 0.0 {
            let derived = amount_quote / fill_price;
            tracing::warn!(
                symbol,
                amount_quote,
                fill_price,
                derived,
                "broker returned filled=0 despite successful order — deriving amount_base from amount spent ÷ price"
            );
            derived
        } else {
            filled_base
        };

        // Live SELL: record realized P&L now from the pre-sell average cost. recompute_realized()
        // below refines this from the full history when the buy legs are present; when they are not,
        // realized_pnl_walk() leaves this value intact (instead of clobbering it with 0).
        let realized = if matches!(mode, TradingMode::Live)
            && side == Action::Sell
            && status == "filled"
            && live_sell_basis > 0.0
            && fill_price > 0.0
        {
            (fill_price - live_sell_basis) * filled_base
        } else {
            realized
        };

        let trade = TradeRecord {
            id: 0,
            account_id,
            decision_id,
            symbol: symbol.to_string(),
            quote: quote.to_string(),
            side,
            mode,
            simulated,
            amount_base: filled_base,
            amount_quote: if status == "filled" {
                filled_base * fill_price
            } else {
                amount_quote
            },
            price: fill_price,
            status: status.clone(),
            external_order_id: ext_id,
            note,
            realized_pnl: realized,
            created_at: Utc::now(),
        };
        let id = self.trades.save(&trade).await?;
        let mut trade = TradeRecord { id, ..trade };
        // live: broker does not report P&L per order → recompute realized P&L from this coin's timeline
        // (idempotent) so Dashboard wins/losses/closed positions are accurate immediately after closing
        if matches!(mode, TradingMode::Live) && status == "filled" {
            match self.trades.recompute_realized(account_id, symbol).await {
                Ok(_) => {
                    if side == Action::Sell {
                        if let Ok(rows) = self.trades.for_symbol(account_id, symbol, 5).await {
                            if let Some(latest) = rows.into_iter().find(|t| t.id == id) {
                                trade.realized_pnl = latest.realized_pnl;
                            }
                        }
                    }
                }
                Err(e) => tracing::warn!(symbol, "failed to recompute realized P&L after closing position: {e}"),
            }
        }
        self.events.publish(&LiveEvent::Trade {
            trade: trade.clone(),
        });
        if status != "filled" {
            self.alert(
                account_id,
                "error",
                "order_failed",
                format!(
                    "{} order for {symbol} failed: {}",
                    side.as_str(),
                    trade.note
                ),
            )
            .await;
            return Err(DomainError::Broker(trade.note));
        }
        if let Some(did) = decision_id {
            if let Err(e) = self
                .repo
                .mark_executed(
                    did,
                    &format!(
                        "{} order filled for {} at {:.8}",
                        side.as_str(),
                        symbol,
                        fill_price
                    ),
                )
                .await
            {
                tracing::warn!(decision_id = did, "failed to mark decision executed: {e}");
            }
        }
        Ok(trade)
    }

    fn to_record(&self, account_id: i64, a: &Analysis, mode: TradingMode) -> DecisionRecord {
        DecisionRecord {
            id: 0,
            account_id,
            symbol: a.symbol.clone(),
            quote: a.quote.clone(),
            mode,
            final_action: a.verdict.action,
            consensus_action: a.consensus.action,
            consensus_confidence: a.consensus.confidence,
            agreement: a.consensus.agreement,
            voted: a.consensus.voted,
            vetoed: a.consensus.vetoed,
            judge_engine: a.verdict.engine.clone(),
            judge_reasoning: a.verdict.reasoning.clone(),
            last_price: a.last_price,
            executed: false,
            note: String::new(),
            created_at: Utc::now(),
        }
    }
}

pub fn default_settings() -> TradingSettings {
    TradingSettings {
        mode: TradingMode::Paper,
        auto_trade: false,
        trade_amount_quote: 1000.0,
        max_position_pct: 0.10,
        min_confidence: 0.65,
        daily_loss_limit: 0.05,
        max_open_positions: 5,
        allow_sell: true,
        take_profit_pct: 0.0,
        // 3.5% hard stop is always on by default (Balanced); 0 here would mean "no stop" and let a
        // position bleed until the AI decides to sell. The catastrophic cap (MAX_LOSS_PCT) bounds
        // the worst case even tighter regardless of this value.
        stop_loss_pct: 0.035,
        discovery_enabled: false,
        discovery_top_n: 5,
        paused: false,
        ai_judge_enabled: true,
        ai_judge_provider: "ollama".into(),
        ai_judge_model: "qwen3:14b".into(),
        ai_judge_ollama_url: "http://localhost:11434".into(),
        ai_judge_base_url: String::new(),
        ai_judge_thinking: true,
        broker: "bitkub".into(),
        manage_style: "conservative".into(),
        let_winners_run: true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reward_risk_accepts_good_long_plan() {
        let rr = validate_long_plan(100.0, 106.0, 96.0).expect("valid RR");
        assert!((rr - 1.5).abs() < 1e-9);
    }

    #[test]
    fn reward_risk_blocks_poor_or_inverted_plan() {
        assert!(validate_long_plan(100.0, 102.0, 98.0).is_err());
        assert!(validate_long_plan(100.0, 99.0, 96.0).is_err());
        assert!(validate_long_plan(100.0, 106.0, 101.0).is_err());
    }

    #[test]
    fn marketable_limit_enters_immediately_instead_of_double_confirming() {
        assert!(should_enter_immediately("limit", 100.0, 100.0, 0.60, 0.45));
        assert!(should_enter_immediately("limit", 99.95, 100.0, 0.60, 0.45));
        assert!(!should_enter_immediately("limit", 98.5, 100.0, 0.60, 0.45));
        assert!(!should_enter_immediately("market", 0.0, 100.0, 0.40, 0.45));
    }

    // ---- managed_stop (trailing / breakeven), conservative preset: breakeven_r=0.7, activate_r=1.0, trail_r=1.0 ----

    fn conservative() -> ManageCfg {
        ManageCfg::from_style("conservative")
    }

    #[test]
    fn managed_stop_holds_when_profit_too_small() {
        // entry 100, initial stop 95 (R=5). At price 101 profit is 0.2R (< 0.7R breakeven) AND
        // below the 2% fixed profit lock → stop unchanged. (At +2% the lock would lift to breakeven.)
        let (stop, active) = managed_stop(100.0, 101.0, 101.0, 95.0, 95.0, conservative());
        assert!((stop - 95.0).abs() < 1e-9, "got {stop}");
        assert!(!active);
    }

    #[test]
    fn managed_stop_moves_to_breakeven() {
        // profit 0.8R (>= 0.7R) but < 1.0R activate → stop lifts to entry + fee buffer (risk-free), not trailing yet
        let (stop, active) = managed_stop(100.0, 104.0, 104.0, 95.0, 95.0, conservative());
        assert!(stop >= 100.0 && stop <= 100.6, "expected ~breakeven, got {stop}");
        assert!(active, "breakeven should mark the plan as managed/risk-free");
    }

    #[test]
    fn managed_stop_trails_below_high_water() {
        // profit 2R (>= 1.0R activate). high-water 110, trail_r=1.0, R=5 → trail = 110 - 5 = 105
        let (stop, active) = managed_stop(100.0, 110.0, 110.0, 95.0, 95.0, conservative());
        assert!((stop - 105.0).abs() < 1e-6, "got {stop}");
        assert!(active);
    }

    #[test]
    fn managed_stop_never_lowers_existing_stop() {
        // price pulled back to 106 but high-water is still 110 and we already trailed to 105 → stop stays 105
        let (stop, _) = managed_stop(100.0, 106.0, 110.0, 95.0, 105.0, conservative());
        assert!((stop - 105.0).abs() < 1e-6, "trailing stop must not move down, got {stop}");
    }

    #[test]
    fn managed_stop_never_at_or_above_price() {
        // huge run: high-water 130, trail would be 125, but price snapped back to 124 → clamp below price
        let (stop, _) = managed_stop(100.0, 124.0, 130.0, 95.0, 105.0, conservative());
        assert!(stop < 124.0, "stop must stay below current price, got {stop}");
    }

    #[test]
    fn managed_stop_disabled_is_noop() {
        let (stop, active) = managed_stop(100.0, 130.0, 130.0, 95.0, 95.0, ManageCfg::from_style("off"));
        assert!((stop - 95.0).abs() < 1e-9);
        assert!(!active);
    }

    #[test]
    fn managed_stop_falls_back_to_pct_risk_without_initial_stop() {
        // no initial stop → R = 2% of basis = 2. profit at 105 = 2.5R (>= activate). trail = high(105) - 1*2 = 103
        let (stop, active) = managed_stop(100.0, 105.0, 105.0, 0.0, 0.0, conservative());
        assert!((stop - 103.0).abs() < 1e-6, "got {stop}");
        assert!(active);
    }

    // ---- fixed-% profit lock (lock_profit_pct): protect gains regardless of how wide R is ----

    #[test]
    fn profit_lock_lifts_to_breakeven_with_a_wide_stop() {
        // entry 100, very wide initial stop 80 (R=20) → R-based breakeven (0.7R) only at +14%.
        // But at just +3% the fixed 2% lock must already make the trade risk-free (stop ~= entry+fees).
        let (stop, active) = managed_stop(100.0, 103.0, 103.0, 80.0, 80.0, conservative());
        assert!(stop >= 100.0 && stop <= 100.6, "expected ~breakeven from profit lock, got {stop}");
        assert!(active);
    }

    #[test]
    fn profit_lock_does_not_trigger_below_threshold() {
        // +1% gain is below the 2% conservative lock → stop stays where it was (no premature lift)
        let (stop, _) = managed_stop(100.0, 101.0, 101.0, 80.0, 80.0, conservative());
        assert!((stop - 80.0).abs() < 1e-9, "stop should not move yet, got {stop}");
    }

    #[test]
    fn profit_lock_threshold_matches_preset() {
        // sanity-check the presets carry a lock and 'off' carries none
        assert_eq!(ManageCfg::from_style("off").lock_profit_pct, 0.0);
        assert!(ManageCfg::from_style("balanced").lock_profit_pct > 0.0);
        assert!(ManageCfg::from_style("conservative").lock_profit_pct > 0.0);
    }
}
