//! Governor — capital/risk controller that translates numbers into human-readable state
//!
//! Answers: what is the bot doing right now? how many more buys are allowed? why isn't it buying?
//! (pure logic — testable without DB/broker)

use crate::domain::models::{GovernorState, PortfolioSnapshot, TradingMode, TradingSettings};

/// Evaluate the governor state for an account from settings + portfolio snapshot + available cash
pub fn evaluate(
    account_id: i64,
    s: &TradingSettings,
    snap: &PortfolioSnapshot,
    cash: f64,
) -> GovernorState {
    let max_open = s.max_open_positions.max(0) as i64;
    let open = snap.open_positions as i64;
    let open_slots = (max_open - open).max(0);
    let buys_by_cash = if s.trade_amount_quote > 0.0 {
        (cash / s.trade_amount_quote).floor() as i64
    } else {
        0
    };
    let buys_remaining = open_slots.min(buys_by_cash).max(0);
    let watch_capacity = max_open.max(1);

    let loss_used = if s.daily_loss_limit > 0.0 && snap.daily_pnl_pct < 0.0 {
        (-snap.daily_pnl_pct / s.daily_loss_limit).clamp(0.0, 1.0)
    } else {
        0.0
    };

    let pnl_pct = snap.daily_pnl_pct * 100.0;
    let limit_pct = s.daily_loss_limit * 100.0;

    // State priority order
    let (state, reason) = if s.paused {
        (
            "paused",
            "⏸️ Paused — you triggered the kill-switch manually (resume to trade again)".to_string(),
        )
    } else if s.daily_loss_limit > 0.0 && snap.daily_pnl_pct <= -s.daily_loss_limit {
        (
            "halted",
            format!(
                "🛑 Trading halted: daily loss limit of {limit_pct:.1}% reached (current {pnl_pct:.1}%) — reset the session to restart"
            ),
        )
    } else if matches!(s.mode, TradingMode::SignalOnly) {
        (
            "signal",
            "📡 Signal-only mode: analysis only, no orders will be placed".to_string(),
        )
    } else if !s.auto_trade {
        (
            "manual",
            "✋ Auto-trade disabled — manual orders only (enable auto-trade in settings)".to_string(),
        )
    } else if open >= max_open && max_open > 0 {
        (
            "full",
            format!("📦 All {max_open} position slots filled — monitoring for take-profit/stop-loss, no new entries"),
        )
    } else if buys_remaining <= 0 {
        (
            "full",
            format!(
                "💸 Insufficient cash to open a new position (need ~{:.0} per position, have {:.0}) — monitoring existing positions",
                s.trade_amount_quote, cash
            ),
        )
    } else {
        (
            "scanning",
            format!(
                "🔍 Scanning for entry — {buys_remaining} buy(s) remaining · watching up to ~{watch_capacity} symbols simultaneously"
            ),
        )
    };

    GovernorState {
        account_id,
        state: state.to_string(),
        reason,
        cash,
        equity: snap.equity,
        daily_pnl_pct: snap.daily_pnl_pct,
        loss_limit: s.daily_loss_limit,
        loss_used,
        open_positions: open,
        max_open_positions: max_open,
        open_slots,
        buys_remaining,
        trade_amount: s.trade_amount_quote,
        auto_trade: s.auto_trade,
        paused: s.paused,
        watch_capacity,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::application::trading::default_settings;

    fn snap(equity: f64, pnl: f64, open: usize) -> PortfolioSnapshot {
        PortfolioSnapshot {
            equity,
            daily_pnl_pct: pnl,
            open_positions: open,
            cash_thb: equity,
            deployed_pct: 0.0,
            session_pnl_pct: pnl,
        }
    }

    #[test]
    fn halts_on_loss_limit() {
        let mut s = default_settings();
        s.auto_trade = true;
        s.daily_loss_limit = 0.05;
        let g = evaluate(1, &s, &snap(95_000.0, -0.06, 0), 95_000.0);
        assert_eq!(g.state, "halted");
        assert!(g.loss_used >= 1.0);
    }

    #[test]
    fn paused_beats_everything() {
        let mut s = default_settings();
        s.auto_trade = true;
        s.paused = true;
        let g = evaluate(1, &s, &snap(50_000.0, -0.10, 0), 50_000.0);
        assert_eq!(g.state, "paused");
    }

    #[test]
    fn scanning_reports_buys_remaining() {
        let mut s = default_settings();
        s.auto_trade = true;
        s.trade_amount_quote = 1000.0;
        s.max_open_positions = 5;
        // cash 3000 → 3 buys possible, 5 open slots available → min = 3
        let g = evaluate(1, &s, &snap(3000.0, 0.0, 0), 3000.0);
        assert_eq!(g.state, "scanning");
        assert_eq!(g.buys_remaining, 3);
    }

    #[test]
    fn full_when_slots_used() {
        let mut s = default_settings();
        s.auto_trade = true;
        s.max_open_positions = 2;
        let g = evaluate(1, &s, &snap(100_000.0, 0.0, 2), 100_000.0);
        assert_eq!(g.state, "full");
        assert_eq!(g.buys_remaining, 0);
    }

    #[test]
    fn manual_when_auto_off() {
        let s = default_settings(); // auto_trade=false
        let g = evaluate(1, &s, &snap(100_000.0, 0.0, 0), 100_000.0);
        assert_eq!(g.state, "manual");
    }
}
