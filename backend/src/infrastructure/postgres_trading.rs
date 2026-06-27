//! PostgreSQL adapters (per-account): SettingsStore, PaperWallet, TradeRepository,
//! PlanRepository, WatchlistStore — implemented on top of the existing PgStore

use async_trait::async_trait;
use sqlx::Row;

use chrono::{DateTime, Utc};

use super::postgres::PgStore;
use crate::domain::models::{
    Action, AlertRecord, FillResult, PaperPosition, PlanState, TradePlan, TradeRecord, TradeStats,
    TradingMode, TradingSettings,
};
use crate::domain::ports::{
    AlertStore, DomainError, DomainResult, PaperWallet, PlanRepository, SettingsStore,
    TradeRepository, WatchlistStore,
};

/// A single trade leg used to recompute realized P&L
struct RealizedLeg {
    id: i64,
    side: String,
    amount_base: f64,
    amount_quote: f64,
    price: f64,
    realized_pnl: f64,
}

/// Walk the timeline of filled legs (already sorted by time), compute a moving-average cost basis,
/// and return (id, new_realized) only for SELL legs whose value differs from the stored one (so we only write to DB when necessary)
fn realized_pnl_walk(legs: &[RealizedLeg]) -> Vec<(i64, f64)> {
    let mut base = 0.0_f64; // number of coins held at this point
    let mut cost = 0.0_f64; // remaining total cost basis
    let mut updates = Vec::new();
    for leg in legs {
        if leg.amount_base <= 0.0 || leg.price <= 0.0 {
            continue;
        }
        if leg.side.eq_ignore_ascii_case("BUY") {
            base += leg.amount_base;
            cost += if leg.amount_quote > 0.0 {
                leg.amount_quote
            } else {
                leg.amount_base * leg.price
            };
        } else if leg.side.eq_ignore_ascii_case("SELL") {
            if base > 0.0 {
                // Real cost basis available → compute accurate realized (overrides any estimate)
                let avg = cost / base;
                let sold = leg.amount_base.min(base);
                let realized = (leg.price - avg) * sold;
                base -= sold;
                cost -= avg * sold;
                if (leg.realized_pnl - realized).abs() > 1e-6 {
                    updates.push((leg.id, realized));
                }
            }
            // base == 0 (no buy history seen, e.g. incomplete sync): leave the stored realized_pnl
            // untouched so a correct sell-time value (computed from the live position avg in
            // execute()) is never clobbered with a phantom 0
        }
    }
    updates
}

fn mode_str(m: TradingMode) -> &'static str {
    match m {
        TradingMode::Paper => "paper",
        TradingMode::Live => "live",
        TradingMode::SignalOnly => "signal-only",
    }
}
fn parse_mode(s: &str) -> TradingMode {
    match s {
        "live" => TradingMode::Live,
        "paper" => TradingMode::Paper,
        _ => TradingMode::SignalOnly,
    }
}

// ================= SettingsStore (per-account) =================

#[async_trait]
impl SettingsStore for PgStore {
    async fn get(&self, account_id: i64) -> DomainResult<TradingSettings> {
        let r = sqlx::query("SELECT * FROM account_settings WHERE account_id=$1")
            .bind(account_id)
            .fetch_one(self.pool())
            .await
            .map_err(|e| DomainError::Repo(e.to_string()))?;
        Ok(TradingSettings {
            mode: parse_mode(r.get::<String, _>("mode").as_str()),
            auto_trade: r.get("auto_trade"),
            trade_amount_quote: r.get("trade_amount_quote"),
            max_position_pct: r.get("max_position_pct"),
            min_confidence: r.get("min_confidence"),
            daily_loss_limit: r.get("daily_loss_limit"),
            max_open_positions: r.get("max_open_positions"),
            allow_sell: r.get("allow_sell"),
            take_profit_pct: r.get("take_profit_pct"),
            stop_loss_pct: r.get("stop_loss_pct"),
            discovery_enabled: r.get("discovery_enabled"),
            discovery_top_n: r.get("discovery_top_n"),
            paused: r.get("paused"),
            ai_judge_enabled: r.get("ai_judge_enabled"),
            ai_judge_provider: r.get("ai_judge_provider"),
            ai_judge_model: r.get("ai_judge_model"),
            ai_judge_ollama_url: r.get("ai_judge_ollama_url"),
            ai_judge_base_url: r.get("ai_judge_base_url"),
            ai_judge_thinking: r.get("ai_judge_thinking"),
            broker: r.get("broker"),
            manage_style: r.get("manage_style"),
            let_winners_run: r.get("let_winners_run"),
        })
    }

    async fn set(&self, account_id: i64, s: &TradingSettings) -> DomainResult<()> {
        sqlx::query(
            r#"UPDATE account_settings SET
                 mode=$2, auto_trade=$3, trade_amount_quote=$4, max_position_pct=$5,
                 min_confidence=$6, daily_loss_limit=$7, max_open_positions=$8, allow_sell=$9,
                 take_profit_pct=$10, stop_loss_pct=$11, discovery_enabled=$12, discovery_top_n=$13,
                 paused=$14, ai_judge_enabled=$15, ai_judge_provider=$16, ai_judge_model=$17,
                 ai_judge_ollama_url=$18, ai_judge_base_url=$19, ai_judge_thinking=$20,
                 broker=$21, manage_style=$22, let_winners_run=$23,
                 updated_at=now()
               WHERE account_id=$1"#,
        )
        .bind(account_id)
        .bind(mode_str(s.mode))
        .bind(s.auto_trade)
        .bind(s.trade_amount_quote)
        .bind(s.max_position_pct)
        .bind(s.min_confidence)
        .bind(s.daily_loss_limit)
        .bind(s.max_open_positions)
        .bind(s.allow_sell)
        .bind(s.take_profit_pct)
        .bind(s.stop_loss_pct)
        .bind(s.discovery_enabled)
        .bind(s.discovery_top_n)
        .bind(s.paused)
        .bind(s.ai_judge_enabled)
        .bind(&s.ai_judge_provider)
        .bind(&s.ai_judge_model)
        .bind(&s.ai_judge_ollama_url)
        .bind(&s.ai_judge_base_url)
        .bind(s.ai_judge_thinking)
        .bind(&s.broker)
        .bind(&s.manage_style)
        .bind(s.let_winners_run)
        .execute(self.pool())
        .await
        .map_err(|e| DomainError::Repo(e.to_string()))?;
        Ok(())
    }

    async fn set_paused(&self, account_id: i64, paused: bool) -> DomainResult<()> {
        sqlx::query("UPDATE account_settings SET paused=$2, updated_at=now() WHERE account_id=$1")
            .bind(account_id)
            .bind(paused)
            .execute(self.pool())
            .await
            .map_err(|e| DomainError::Repo(e.to_string()))?;
        Ok(())
    }
}

// ================= AlertStore (per-account) =================

#[async_trait]
impl AlertStore for PgStore {
    async fn save(&self, a: &AlertRecord) -> DomainResult<i64> {
        let r = sqlx::query(
            "INSERT INTO alerts (account_id, level, code, message, created_at)
             VALUES ($1,$2,$3,$4,$5) RETURNING id",
        )
        .bind(a.account_id)
        .bind(&a.level)
        .bind(&a.code)
        .bind(&a.message)
        .bind(a.created_at)
        .fetch_one(self.pool())
        .await
        .map_err(|e| DomainError::Repo(e.to_string()))?;
        Ok(r.get("id"))
    }

    async fn recent(&self, account_id: i64, limit: i64) -> DomainResult<Vec<AlertRecord>> {
        let rows = sqlx::query(
            "SELECT * FROM alerts WHERE account_id=$1 ORDER BY created_at DESC LIMIT $2",
        )
        .bind(account_id)
        .bind(limit.clamp(1, 500))
        .fetch_all(self.pool())
        .await
        .map_err(|e| DomainError::Repo(e.to_string()))?;
        Ok(rows
            .iter()
            .map(|r| AlertRecord {
                id: r.get("id"),
                account_id: r.get("account_id"),
                level: r.get("level"),
                code: r.get("code"),
                message: r.get("message"),
                created_at: r.get("created_at"),
            })
            .collect())
    }
}

// ================= WatchlistStore (per-account) =================

#[async_trait]
impl WatchlistStore for PgStore {
    async fn get_symbols(&self, account_id: i64) -> DomainResult<Vec<String>> {
        let rows = sqlx::query(
            "SELECT symbol FROM watch_symbols WHERE account_id=$1 ORDER BY sort_order, symbol",
        )
        .bind(account_id)
        .fetch_all(self.pool())
        .await
        .map_err(|e| DomainError::Repo(e.to_string()))?;
        Ok(rows.iter().map(|r| r.get("symbol")).collect())
    }

    async fn set_symbols(&self, account_id: i64, symbols: &[String]) -> DomainResult<()> {
        let mut tx = self
            .pool()
            .begin()
            .await
            .map_err(|e| DomainError::Repo(e.to_string()))?;
        sqlx::query("DELETE FROM watch_symbols WHERE account_id=$1")
            .bind(account_id)
            .execute(&mut *tx)
            .await
            .map_err(|e| DomainError::Repo(e.to_string()))?;
        for (idx, sym) in symbols.iter().enumerate() {
            sqlx::query(
                r#"INSERT INTO watch_symbols (account_id, symbol, sort_order, updated_at)
                   VALUES ($1, $2, $3, now())"#,
            )
            .bind(account_id)
            .bind(sym)
            .bind(idx as i32)
            .execute(&mut *tx)
            .await
            .map_err(|e| DomainError::Repo(e.to_string()))?;
        }
        tx.commit()
            .await
            .map_err(|e| DomainError::Repo(e.to_string()))?;
        Ok(())
    }
}

// ================= PaperWallet (per-account) =================

#[async_trait]
impl PaperWallet for PgStore {
    async fn view(&self, account_id: i64) -> DomainResult<(f64, f64, Vec<PaperPosition>)> {
        // fetch_optional: some accounts (especially legacy live accounts) may not yet have a wallet row → avoid 500 error
        let w =
            sqlx::query("SELECT cash_quote, starting_cash FROM account_wallet WHERE account_id=$1")
                .bind(account_id)
                .fetch_optional(self.pool())
                .await
                .map_err(|e| DomainError::Repo(e.to_string()))?;
        let (cash, starting): (f64, f64) = match w {
            Some(r) => (r.get("cash_quote"), r.get("starting_cash")),
            None => (100_000.0, 100_000.0),
        };
        let rows = sqlx::query(
            "SELECT symbol, amount_base, avg_price FROM paper_positions WHERE account_id=$1 AND amount_base > 0 ORDER BY symbol",
        )
        .bind(account_id)
        .fetch_all(self.pool())
        .await
        .map_err(|e| DomainError::Repo(e.to_string()))?;
        let positions = rows
            .iter()
            .map(|r| PaperPosition {
                symbol: r.get("symbol"),
                amount_base: r.get("amount_base"),
                avg_price: r.get("avg_price"),
                last_price: 0.0,
            })
            .collect();
        Ok((cash, starting, positions))
    }

    async fn reset(&self, account_id: i64, starting_cash: f64) -> DomainResult<()> {
        let mut tx = self
            .pool()
            .begin()
            .await
            .map_err(|e| DomainError::Repo(e.to_string()))?;
        sqlx::query("UPDATE account_wallet SET cash_quote=$2, starting_cash=$2, session_start=now(), updated_at=now() WHERE account_id=$1")
            .bind(account_id)
            .bind(starting_cash)
            .execute(&mut *tx)
            .await
            .map_err(|e| DomainError::Repo(e.to_string()))?;
        sqlx::query("DELETE FROM paper_positions WHERE account_id=$1")
            .bind(account_id)
            .execute(&mut *tx)
            .await
            .map_err(|e| DomainError::Repo(e.to_string()))?;
        tx.commit()
            .await
            .map_err(|e| DomainError::Repo(e.to_string()))?;
        Ok(())
    }

    async fn session_start(&self, account_id: i64) -> DomainResult<DateTime<Utc>> {
        let r = sqlx::query("SELECT session_start FROM account_wallet WHERE account_id=$1")
            .bind(account_id)
            .fetch_optional(self.pool())
            .await
            .map_err(|e| DomainError::Repo(e.to_string()))?;
        // live accounts without a wallet row → use epoch (count stats from the beginning)
        Ok(r.map(|r| r.get("session_start"))
            .unwrap_or_else(|| DateTime::<Utc>::from_timestamp(0, 0).unwrap()))
    }

    async fn reset_session(&self, account_id: i64) -> DomainResult<()> {
        sqlx::query(
            "UPDATE account_wallet SET session_start=now(), updated_at=now() WHERE account_id=$1",
        )
        .bind(account_id)
        .execute(self.pool())
        .await
        .map_err(|e| DomainError::Repo(e.to_string()))?;
        Ok(())
    }

    async fn apply_fill(
        &self,
        account_id: i64,
        symbol: &str,
        side_buy: bool,
        amount_quote: f64,
        price: f64,
    ) -> DomainResult<FillResult> {
        if price <= 0.0 {
            return Err(DomainError::Broker("invalid price (0)".into()));
        }
        let mut tx = self
            .pool()
            .begin()
            .await
            .map_err(|e| DomainError::Repo(e.to_string()))?;
        let cash: f64 =
            sqlx::query("SELECT cash_quote FROM account_wallet WHERE account_id=$1 FOR UPDATE")
                .bind(account_id)
                .fetch_one(&mut *tx)
                .await
                .map_err(|e| DomainError::Repo(e.to_string()))?
                .get("cash_quote");

        let existing = sqlx::query(
            "SELECT amount_base, avg_price FROM paper_positions WHERE account_id=$1 AND symbol=$2 FOR UPDATE",
        )
        .bind(account_id)
        .bind(symbol)
        .fetch_optional(&mut *tx)
        .await
        .map_err(|e| DomainError::Repo(e.to_string()))?;
        let (mut amt, mut avg) = existing
            .map(|r| (r.get::<f64, _>("amount_base"), r.get::<f64, _>("avg_price")))
            .unwrap_or((0.0, 0.0));

        let filled_base;
        let new_cash;
        let mut realized_pnl = 0.0;
        if side_buy {
            let spend = amount_quote.min(cash);
            if spend <= 0.0 {
                return Err(DomainError::RiskBlocked("insufficient cash balance".into()));
            }
            filled_base = spend / price;
            avg = if amt + filled_base > 0.0 {
                (amt * avg + filled_base * price) / (amt + filled_base)
            } else {
                price
            };
            amt += filled_base;
            new_cash = cash - spend;
        } else {
            let want_base = amount_quote / price;
            filled_base = want_base.min(amt);
            if filled_base <= 0.0 {
                return Err(DomainError::RiskBlocked(format!("no {symbol} available to sell")));
            }
            realized_pnl = (price - avg) * filled_base;
            amt -= filled_base;
            new_cash = cash + filled_base * price;
        }

        sqlx::query(
            "UPDATE account_wallet SET cash_quote=$2, updated_at=now() WHERE account_id=$1",
        )
        .bind(account_id)
        .bind(new_cash)
        .execute(&mut *tx)
        .await
        .map_err(|e| DomainError::Repo(e.to_string()))?;
        sqlx::query(
            r#"INSERT INTO paper_positions (account_id, symbol, amount_base, avg_price, updated_at)
               VALUES ($1,$2,$3,$4, now())
               ON CONFLICT (account_id, symbol) DO UPDATE SET amount_base=$3, avg_price=$4, updated_at=now()"#,
        )
        .bind(account_id)
        .bind(symbol)
        .bind(amt)
        .bind(avg)
        .execute(&mut *tx)
        .await
        .map_err(|e| DomainError::Repo(e.to_string()))?;
        tx.commit()
            .await
            .map_err(|e| DomainError::Repo(e.to_string()))?;
        Ok(FillResult {
            filled_base,
            realized_pnl,
        })
    }

    async fn position(&self, account_id: i64, symbol: &str) -> DomainResult<Option<PaperPosition>> {
        let r = sqlx::query("SELECT symbol, amount_base, avg_price FROM paper_positions WHERE account_id=$1 AND symbol=$2 AND amount_base > 0")
            .bind(account_id)
            .bind(symbol)
            .fetch_optional(self.pool())
            .await
            .map_err(|e| DomainError::Repo(e.to_string()))?;
        Ok(r.map(|r| PaperPosition {
            symbol: r.get("symbol"),
            amount_base: r.get("amount_base"),
            avg_price: r.get("avg_price"),
            last_price: 0.0,
        }))
    }
}

// ================= TradeRepository (per-account) =================

fn row_to_trade(r: &sqlx::postgres::PgRow) -> TradeRecord {
    TradeRecord {
        id: r.get("id"),
        account_id: r.get("account_id"),
        decision_id: r.get("decision_id"),
        symbol: r.get("symbol"),
        quote: r.get("quote"),
        side: Action::parse(r.get::<String, _>("side").as_str()),
        mode: parse_mode(r.get::<String, _>("mode").as_str()),
        simulated: r.get("simulated"),
        amount_base: r.get("amount_base"),
        amount_quote: r.get("amount_quote"),
        price: r.get("price"),
        status: r.get("status"),
        external_order_id: r.get("external_order_id"),
        note: r.get("note"),
        realized_pnl: r.get("realized_pnl"),
        created_at: r.get("created_at"),
    }
}

#[async_trait]
impl TradeRepository for PgStore {
    async fn save(&self, t: &TradeRecord) -> DomainResult<i64> {
        let id: i64 = sqlx::query(
            r#"INSERT INTO trades
               (account_id, decision_id, symbol, quote, side, mode, simulated, amount_base, amount_quote,
                price, status, external_order_id, note, realized_pnl, created_at)
               VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15) RETURNING id"#,
        )
        .bind(t.account_id)
        .bind(t.decision_id)
        .bind(&t.symbol)
        .bind(&t.quote)
        .bind(t.side.as_str())
        .bind(mode_str(t.mode))
        .bind(t.simulated)
        .bind(t.amount_base)
        .bind(t.amount_quote)
        .bind(t.price)
        .bind(&t.status)
        .bind(&t.external_order_id)
        .bind(&t.note)
        .bind(t.realized_pnl)
        .bind(t.created_at)
        .fetch_one(self.pool())
        .await
        .map_err(|e| DomainError::Repo(e.to_string()))?
        .get("id");
        Ok(id)
    }

    async fn save_external(&self, t: &TradeRecord) -> DomainResult<i64> {
        if t.external_order_id.trim().is_empty() {
            return TradeRepository::save(self, t).await;
        }
        let id: i64 = sqlx::query(
            r#"INSERT INTO trades
               (account_id, decision_id, symbol, quote, side, mode, simulated, amount_base, amount_quote,
                price, status, external_order_id, note, realized_pnl, created_at)
               VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15)
               ON CONFLICT (account_id, external_order_id) WHERE external_order_id <> '' DO UPDATE SET
                 decision_id=COALESCE(trades.decision_id, EXCLUDED.decision_id),
                 symbol=EXCLUDED.symbol,
                 quote=EXCLUDED.quote,
                 side=EXCLUDED.side,
                 mode=EXCLUDED.mode,
                 simulated=EXCLUDED.simulated,
                 amount_base=CASE WHEN trades.amount_base <= 0 THEN EXCLUDED.amount_base ELSE trades.amount_base END,
                 amount_quote=CASE WHEN trades.amount_quote <= 0 THEN EXCLUDED.amount_quote ELSE trades.amount_quote END,
                 price=CASE WHEN trades.price <= 0 THEN EXCLUDED.price ELSE trades.price END,
                 status=EXCLUDED.status,
                 note=CASE WHEN trades.note = '' OR trades.note LIKE 'bitkub%' THEN EXCLUDED.note ELSE trades.note END,
                 realized_pnl=trades.realized_pnl,
                 created_at=LEAST(trades.created_at, EXCLUDED.created_at)
               RETURNING id"#,
        )
        .bind(t.account_id)
        .bind(t.decision_id)
        .bind(&t.symbol)
        .bind(&t.quote)
        .bind(t.side.as_str())
        .bind(mode_str(t.mode))
        .bind(t.simulated)
        .bind(t.amount_base)
        .bind(t.amount_quote)
        .bind(t.price)
        .bind(&t.status)
        .bind(&t.external_order_id)
        .bind(&t.note)
        .bind(t.realized_pnl)
        .bind(t.created_at)
        .fetch_one(self.pool())
        .await
        .map_err(|e| DomainError::Repo(e.to_string()))?
        .get("id");
        Ok(id)
    }

    async fn recent(&self, account_id: i64, limit: i64) -> DomainResult<Vec<TradeRecord>> {
        let rows = sqlx::query(
            "SELECT * FROM trades WHERE account_id=$1 ORDER BY created_at DESC LIMIT $2",
        )
        .bind(account_id)
        .bind(limit)
        .fetch_all(self.pool())
        .await
        .map_err(|e| DomainError::Repo(e.to_string()))?;
        Ok(rows.iter().map(row_to_trade).collect())
    }

    async fn for_symbol(
        &self,
        account_id: i64,
        symbol: &str,
        limit: i64,
    ) -> DomainResult<Vec<TradeRecord>> {
        let rows = sqlx::query(
            "SELECT * FROM trades WHERE account_id=$1 AND symbol=$2 ORDER BY created_at DESC LIMIT $3",
        )
        .bind(account_id)
        .bind(symbol)
        .bind(limit)
        .fetch_all(self.pool())
        .await
        .map_err(|e| DomainError::Repo(e.to_string()))?;
        Ok(rows.iter().map(row_to_trade).collect())
    }

    async fn symbols_with_zero_amount(&self, account_id: i64) -> DomainResult<Vec<String>> {
        let rows = sqlx::query(
            "SELECT DISTINCT symbol FROM trades
             WHERE account_id=$1 AND status='filled' AND amount_base <= 0 AND external_order_id <> ''",
        )
        .bind(account_id)
        .fetch_all(self.pool())
        .await
        .map_err(|e| DomainError::Repo(e.to_string()))?;
        Ok(rows.iter().map(|r| r.get::<String, _>("symbol")).collect())
    }

    async fn distinct_symbols(&self, account_id: i64) -> DomainResult<Vec<String>> {
        let rows = sqlx::query(
            "SELECT DISTINCT symbol FROM trades WHERE account_id=$1 AND status='filled'",
        )
        .bind(account_id)
        .fetch_all(self.pool())
        .await
        .map_err(|e| DomainError::Repo(e.to_string()))?;
        Ok(rows.iter().map(|r| r.get::<String, _>("symbol")).collect())
    }

    async fn recompute_realized(&self, account_id: i64, symbol: &str) -> DomainResult<u64> {
        // Walk all filled legs for (account, symbol) in chronological order, rebuild the moving-average cost basis,
        // then compute realized = (sell price - avg cost at that point) * amount sold for each SELL leg
        let rows = sqlx::query(
            "SELECT id, side, amount_base, amount_quote, price, realized_pnl
             FROM trades
             WHERE account_id=$1 AND symbol=$2 AND status='filled'
             ORDER BY created_at ASC, id ASC",
        )
        .bind(account_id)
        .bind(symbol)
        .fetch_all(self.pool())
        .await
        .map_err(|e| DomainError::Repo(e.to_string()))?;

        let legs: Vec<RealizedLeg> = rows
            .iter()
            .map(|r| RealizedLeg {
                id: r.get("id"),
                side: r.get("side"),
                amount_base: r.get("amount_base"),
                amount_quote: r.get("amount_quote"),
                price: r.get("price"),
                realized_pnl: r.get("realized_pnl"),
            })
            .collect();

        let updates = realized_pnl_walk(&legs);
        for (id, realized) in &updates {
            sqlx::query("UPDATE trades SET realized_pnl=$2 WHERE id=$1")
                .bind(id)
                .bind(realized)
                .execute(self.pool())
                .await
                .map_err(|e| DomainError::Repo(e.to_string()))?;
        }
        Ok(updates.len() as u64)
    }

    async fn position_basis(&self, account_id: i64, symbol: &str) -> DomainResult<(f64, f64)> {
        // Walk all filled legs chronologically with a moving-average cost basis (same accounting
        // as realized_pnl_walk) and return the currently-held (amount, avg_cost) from the LOCAL
        // ledger — so realized P&L can be recorded at sell time even when the broker reports no avg.
        let rows = sqlx::query(
            "SELECT side, amount_base, amount_quote, price
             FROM trades
             WHERE account_id=$1 AND symbol=$2 AND status='filled'
             ORDER BY created_at ASC, id ASC",
        )
        .bind(account_id)
        .bind(symbol)
        .fetch_all(self.pool())
        .await
        .map_err(|e| DomainError::Repo(e.to_string()))?;

        let mut base = 0.0_f64; // coins currently held
        let mut cost = 0.0_f64; // total remaining cost basis
        for r in &rows {
            let side: String = r.get("side");
            let amount_base: f64 = r.get("amount_base");
            let amount_quote: f64 = r.get("amount_quote");
            let price: f64 = r.get("price");
            if amount_base <= 0.0 || price <= 0.0 {
                continue;
            }
            if side.eq_ignore_ascii_case("BUY") {
                base += amount_base;
                cost += if amount_quote > 0.0 {
                    amount_quote
                } else {
                    amount_base * price
                };
            } else if side.eq_ignore_ascii_case("SELL") && base > 0.0 {
                let avg = cost / base;
                let sold = amount_base.min(base);
                base -= sold;
                cost -= avg * sold;
            }
        }
        let avg = if base > 1e-12 { cost / base } else { 0.0 };
        Ok((base.max(0.0), avg))
    }

    async fn paper_stats(&self, account_id: i64, since: DateTime<Utc>) -> DomainResult<TradeStats> {
        let r = sqlx::query(
            r#"SELECT
                 COUNT(*) FILTER (WHERE status='filled')                                  AS total,
                 COUNT(*) FILTER (WHERE status='filled' AND side='BUY')                    AS buys,
                 COUNT(*) FILTER (WHERE status='filled' AND side='SELL')                   AS closed,
                 COUNT(*) FILTER (WHERE status='filled' AND side='SELL' AND realized_pnl > 0)  AS wins,
                 COUNT(*) FILTER (WHERE status='filled' AND side='SELL' AND realized_pnl < 0)  AS losses,
                 COALESCE(SUM(realized_pnl) FILTER (WHERE status='filled'), 0)             AS gross,
                 COALESCE(AVG(realized_pnl) FILTER (WHERE status='filled' AND realized_pnl > 0),0) AS avg_win,
                 COALESCE(AVG(realized_pnl) FILTER (WHERE status='filled' AND realized_pnl < 0),0) AS avg_loss,
                 COALESCE(MAX(realized_pnl) FILTER (WHERE status='filled'),0)              AS best,
                 COALESCE(MIN(realized_pnl) FILTER (WHERE status='filled'),0)              AS worst,
                 COALESCE(SUM(realized_pnl) FILTER (WHERE status='filled' AND realized_pnl > 0),0) AS win_sum,
                 COALESCE(SUM(realized_pnl) FILTER (WHERE status='filled' AND realized_pnl < 0),0) AS loss_sum
               FROM trades WHERE account_id=$1 AND created_at >= $2"#,
        )
        .bind(account_id)
        .bind(since)
        .fetch_one(self.pool())
        .await
        .map_err(|e| DomainError::Repo(e.to_string()))?;

        let wins: i64 = r.get("wins");
        let losses: i64 = r.get("losses");
        // closed = all actual sell legs (including break-even); win_rate is computed separately from wins/(wins+losses)
        let closed: i64 = r.get("closed");
        let decided = wins + losses;
        let loss_sum: f64 = r.get("loss_sum");
        let win_sum: f64 = r.get("win_sum");
        Ok(TradeStats {
            session_start: since,
            total_trades: r.get("total"),
            buys: r.get("buys"),
            closed,
            wins,
            losses,
            win_rate: if decided > 0 {
                wins as f64 / decided as f64
            } else {
                0.0
            },
            gross_pnl: r.get("gross"),
            avg_win: r.get("avg_win"),
            avg_loss: r.get("avg_loss"),
            best: r.get("best"),
            worst: r.get("worst"),
            profit_factor: if loss_sum.abs() > 0.0 {
                win_sum / loss_sum.abs()
            } else {
                0.0
            },
        })
    }

    async fn realized_since(&self, account_id: i64, since: DateTime<Utc>) -> DomainResult<f64> {
        let r = sqlx::query(
            "SELECT COALESCE(SUM(realized_pnl),0) AS pnl FROM trades
             WHERE account_id=$1 AND status='filled' AND created_at >= $2",
        )
        .bind(account_id)
        .bind(since)
        .fetch_one(self.pool())
        .await
        .map_err(|e| DomainError::Repo(e.to_string()))?;
        Ok(r.get("pnl"))
    }

    async fn clear_all(&self, account_id: i64) -> DomainResult<u64> {
        let res = sqlx::query("DELETE FROM trades WHERE account_id=$1")
            .bind(account_id)
            .execute(self.pool())
            .await
            .map_err(|e| DomainError::Repo(e.to_string()))?;
        Ok(res.rows_affected())
    }
}

// ================= PlanRepository (per-account) =================

fn plan_state_str(s: PlanState) -> &'static str {
    match s {
        PlanState::Pending => "pending",
        PlanState::Open => "open",
        PlanState::Closed => "closed",
        PlanState::Cancelled => "cancelled",
    }
}
fn parse_plan_state(s: &str) -> PlanState {
    match s {
        "open" => PlanState::Open,
        "closed" => PlanState::Closed,
        "cancelled" => PlanState::Cancelled,
        _ => PlanState::Pending,
    }
}
fn row_to_plan(r: &sqlx::postgres::PgRow) -> TradePlan {
    TradePlan {
        id: r.get("id"),
        account_id: r.get("account_id"),
        symbol: r.get("symbol"),
        quote: r.get("quote"),
        state: parse_plan_state(r.get::<String, _>("state").as_str()),
        action: Action::parse(r.get::<String, _>("action").as_str()),
        entry_type: r.get("entry_type"),
        entry_price: r.get("entry_price"),
        target_price: r.get("target_price"),
        stop_price: r.get("stop_price"),
        confidence: r.get("confidence"),
        thesis: r.get("thesis"),
        invalidation: r.get("invalidation"),
        next_step: r.get("next_step"),
        decision_id: r.get("decision_id"),
        last_price: r.get("last_price"),
        high_water_mark: r.get("high_water_mark"),
        initial_stop: r.get("initial_stop"),
        trail_active: r.get("trail_active"),
        created_at: r.get("created_at"),
        updated_at: r.get("updated_at"),
    }
}

#[async_trait]
impl PlanRepository for PgStore {
    async fn upsert(&self, p: &TradePlan) -> DomainResult<i64> {
        let id: i64 = sqlx::query(
            r#"INSERT INTO trade_plans
               (account_id, symbol, quote, state, action, entry_type, entry_price, target_price, stop_price,
                confidence, thesis, invalidation, next_step, decision_id, last_price,
                high_water_mark, initial_stop, trail_active, updated_at)
               VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17,$18, now())
               ON CONFLICT (account_id, symbol) DO UPDATE SET
                 quote=$3, state=$4, action=$5, entry_type=$6, entry_price=$7, target_price=$8,
                 stop_price=$9, confidence=$10, thesis=$11, invalidation=$12, next_step=$13,
                 decision_id=$14, last_price=$15, high_water_mark=$16, initial_stop=$17,
                 trail_active=$18, created_at=now(), updated_at=now()
               RETURNING id"#,
        )
        .bind(p.account_id)
        .bind(&p.symbol)
        .bind(&p.quote)
        .bind(plan_state_str(p.state))
        .bind(p.action.as_str())
        .bind(&p.entry_type)
        .bind(p.entry_price)
        .bind(p.target_price)
        .bind(p.stop_price)
        .bind(p.confidence)
        .bind(&p.thesis)
        .bind(&p.invalidation)
        .bind(&p.next_step)
        .bind(p.decision_id)
        .bind(p.last_price)
        .bind(p.high_water_mark)
        .bind(p.initial_stop)
        .bind(p.trail_active)
        .fetch_one(self.pool())
        .await
        .map_err(|e| DomainError::Repo(e.to_string()))?
        .get("id");
        Ok(id)
    }

    async fn active(&self, account_id: i64) -> DomainResult<Vec<TradePlan>> {
        let rows = sqlx::query(
            "SELECT * FROM trade_plans WHERE account_id=$1 AND state IN ('pending','open') ORDER BY updated_at DESC",
        )
        .bind(account_id)
        .fetch_all(self.pool())
        .await
        .map_err(|e| DomainError::Repo(e.to_string()))?;
        Ok(rows.iter().map(row_to_plan).collect())
    }

    async fn get(&self, account_id: i64, symbol: &str) -> DomainResult<Option<TradePlan>> {
        let r = sqlx::query("SELECT * FROM trade_plans WHERE account_id=$1 AND symbol=$2")
            .bind(account_id)
            .bind(symbol)
            .fetch_optional(self.pool())
            .await
            .map_err(|e| DomainError::Repo(e.to_string()))?;
        Ok(r.as_ref().map(row_to_plan))
    }

    async fn set_state(&self, account_id: i64, symbol: &str, state: PlanState) -> DomainResult<()> {
        sqlx::query(
            "UPDATE trade_plans SET state=$3, updated_at=now() WHERE account_id=$1 AND symbol=$2",
        )
        .bind(account_id)
        .bind(symbol)
        .bind(plan_state_str(state))
        .execute(self.pool())
        .await
        .map_err(|e| DomainError::Repo(e.to_string()))?;
        Ok(())
    }

    async fn set_last_price(&self, account_id: i64, symbol: &str, price: f64) -> DomainResult<()> {
        sqlx::query("UPDATE trade_plans SET last_price=$3 WHERE account_id=$1 AND symbol=$2")
            .bind(account_id)
            .bind(symbol)
            .bind(price)
            .execute(self.pool())
            .await
            .map_err(|e| DomainError::Repo(e.to_string()))?;
        Ok(())
    }

    async fn update_trailing(
        &self,
        account_id: i64,
        symbol: &str,
        high_water: f64,
        new_stop: f64,
        trail_active: bool,
    ) -> DomainResult<()> {
        // GREATEST so the stop and high-water mark only ever ratchet up — never lower a managed stop
        sqlx::query(
            "UPDATE trade_plans
             SET high_water_mark = GREATEST(high_water_mark, $3),
                 stop_price      = GREATEST(stop_price, $4),
                 trail_active    = trail_active OR $5,
                 updated_at      = now()
             WHERE account_id=$1 AND symbol=$2",
        )
        .bind(account_id)
        .bind(symbol)
        .bind(high_water)
        .bind(new_stop)
        .bind(trail_active)
        .execute(self.pool())
        .await
        .map_err(|e| DomainError::Repo(e.to_string()))?;
        Ok(())
    }

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
    ) -> DomainResult<()> {
        // Re-analysis of a held position: refresh thesis/target, raise the stop (GREATEST, never lower),
        // but keep high_water_mark / initial_stop / trail_active managed by the trailing logic.
        sqlx::query(
            "UPDATE trade_plans
             SET target_price = $3,
                 stop_price   = GREATEST(stop_price, $4),
                 confidence   = $5,
                 thesis       = $6,
                 invalidation = $7,
                 next_step    = $8,
                 decision_id  = COALESCE($9, decision_id),
                 updated_at   = now()
             WHERE account_id=$1 AND symbol=$2",
        )
        .bind(account_id)
        .bind(symbol)
        .bind(target)
        .bind(new_stop)
        .bind(confidence)
        .bind(thesis)
        .bind(invalidation)
        .bind(next_step)
        .bind(decision_id)
        .execute(self.pool())
        .await
        .map_err(|e| DomainError::Repo(e.to_string()))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{realized_pnl_walk, RealizedLeg};

    fn leg(id: i64, side: &str, base: f64, quote: f64, price: f64, old: f64) -> RealizedLeg {
        RealizedLeg {
            id,
            side: side.into(),
            amount_base: base,
            amount_quote: quote,
            price,
            realized_pnl: old,
        }
    }

    #[test]
    fn buy_then_sell_higher_is_a_win() {
        // buy 2 @ 100 (cost 200), sell 2 @ 120 → profit (120-100)*2 = 40
        let legs = vec![
            leg(1, "BUY", 2.0, 200.0, 100.0, 0.0),
            leg(2, "SELL", 2.0, 240.0, 120.0, 0.0),
        ];
        let u = realized_pnl_walk(&legs);
        assert_eq!(u.len(), 1);
        assert_eq!(u[0].0, 2);
        assert!((u[0].1 - 40.0).abs() < 1e-6, "got {}", u[0].1);
    }

    #[test]
    fn sell_lower_is_a_loss() {
        let legs = vec![
            leg(1, "BUY", 1.0, 100.0, 100.0, 0.0),
            leg(2, "SELL", 1.0, 80.0, 80.0, 0.0),
        ];
        let u = realized_pnl_walk(&legs);
        assert_eq!(u.len(), 1);
        assert!((u[0].1 - (-20.0)).abs() < 1e-6, "got {}", u[0].1);
    }

    #[test]
    fn partial_sell_uses_average_cost() {
        // buy 1@100 then 1@200 → avg cost 150; sell 1@180 → (180-150)*1 = 30
        let legs = vec![
            leg(1, "BUY", 1.0, 100.0, 100.0, 0.0),
            leg(2, "BUY", 1.0, 200.0, 200.0, 0.0),
            leg(3, "SELL", 1.0, 180.0, 180.0, 0.0),
        ];
        let u = realized_pnl_walk(&legs);
        assert_eq!(u.len(), 1);
        assert!((u[0].1 - 30.0).abs() < 1e-6, "got {}", u[0].1);
    }

    #[test]
    fn sell_without_known_buy_yields_zero_not_phantom_profit() {
        // sell with no buy in history (incomplete sync) → realized = 0, not phantom profit
        let legs = vec![leg(1, "SELL", 1.0, 500.0, 500.0, 0.0)];
        let u = realized_pnl_walk(&legs);
        assert!(u.is_empty());
    }

    #[test]
    fn sell_without_known_buy_does_not_clobber_existing_realized() {
        // a live SELL whose realized was already set at sell-time (e.g. 30 from the live position avg)
        // must NOT be overwritten with 0 just because the buy history was never synced
        let legs = vec![leg(1, "SELL", 1.0, 500.0, 500.0, 30.0)];
        let u = realized_pnl_walk(&legs);
        assert!(u.is_empty(), "must preserve the stored sell-time realized P&L");
    }

    #[test]
    fn no_update_when_value_unchanged() {
        // if the stored realized value is already correct → no redundant write
        let legs = vec![
            leg(1, "BUY", 1.0, 100.0, 100.0, 0.0),
            leg(2, "SELL", 1.0, 120.0, 120.0, 20.0),
        ];
        let u = realized_pnl_walk(&legs);
        assert!(u.is_empty(), "value already correct, no update expected");
    }

    #[test]
    fn buy_quote_zero_falls_back_to_base_times_price() {
        // buy leg with amount_quote=0 (broker did not return it) → use base*price as cost basis
        let legs = vec![
            leg(1, "BUY", 2.0, 0.0, 50.0, 0.0),
            leg(2, "SELL", 2.0, 140.0, 70.0, 0.0),
        ];
        let u = realized_pnl_walk(&legs);
        assert_eq!(u.len(), 1);
        assert!((u[0].1 - 40.0).abs() < 1e-6, "got {}", u[0].1);
    }
}
