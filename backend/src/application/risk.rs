//! Risk rules — pure logic, testable without DB/broker

use crate::domain::models::{PortfolioSnapshot, RiskConfig, RiskDecision};

/// Evaluate an order against risk rules — returns the allowed amount cap or the reason it is blocked
pub fn evaluate(cfg: &RiskConfig, pf: &PortfolioSnapshot, requested_quote: f64) -> RiskDecision {
    if pf.daily_pnl_pct <= -cfg.daily_loss_limit {
        return RiskDecision::Block {
            reason: format!(
                "daily loss limit hit ({:.1}% ≤ -{:.1}%)",
                pf.daily_pnl_pct * 100.0,
                cfg.daily_loss_limit * 100.0
            ),
        };
    }
    if pf.open_positions >= cfg.max_open_positions {
        return RiskDecision::Block {
            reason: format!("open position limit reached ({})", cfg.max_open_positions),
        };
    }
    // Honor the user's configured per-order amount. `max_position_pct` is a ceiling that only caps
    // requests LARGER than what the user asked for — it must NEVER shrink an explicit order below the
    // configured size (that surprised the user: a 200 THB order became ~85 on a small account).
    // The real limiter is available cash (also re-checked in preflight).
    let ceiling = (pf.equity * cfg.max_position_pct).max(requested_quote);
    let allowed = requested_quote.min(ceiling).min(pf.cash_thb.max(0.0));
    RiskDecision::Allow {
        max_amount_quote: allowed,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg() -> RiskConfig {
        RiskConfig {
            max_position_pct: 0.10,
            daily_loss_limit: 0.05,
            max_open_positions: 5,
        }
    }

    #[test]
    fn blocks_on_daily_loss() {
        let pf = PortfolioSnapshot {
            equity: 1000.0,
            daily_pnl_pct: -0.06,
            open_positions: 0,
            cash_thb: 1000.0,
            deployed_pct: 0.0,
            session_pnl_pct: -0.06,
        };
        assert!(matches!(
            evaluate(&cfg(), &pf, 100.0),
            RiskDecision::Block { .. }
        ));
    }

    #[test]
    fn honors_requested_amount_even_when_above_position_pct() {
        // user explicitly asked for 500; the 10% position cap (=100) must NOT shrink it. Cash allows it.
        let pf = PortfolioSnapshot {
            equity: 1000.0,
            daily_pnl_pct: 0.0,
            open_positions: 0,
            cash_thb: 1000.0,
            deployed_pct: 0.0,
            session_pnl_pct: 0.0,
        };
        match evaluate(&cfg(), &pf, 500.0) {
            RiskDecision::Allow { max_amount_quote } => assert_eq!(max_amount_quote, 500.0),
            _ => panic!("expected Allow"),
        }
    }

    #[test]
    fn small_account_still_buys_the_configured_order_size() {
        // the real bug report: 200 THB/order on a ~284 THB account was shrunk to ~85 by the 30% cap.
        // now the full 200 is honored because cash (284) covers it.
        let pf = PortfolioSnapshot {
            equity: 284.0,
            daily_pnl_pct: 0.0,
            open_positions: 0,
            cash_thb: 284.0,
            deployed_pct: 0.0,
            session_pnl_pct: 0.0,
        };
        match evaluate(&cfg(), &pf, 200.0) {
            RiskDecision::Allow { max_amount_quote } => assert_eq!(max_amount_quote, 200.0),
            _ => panic!("expected Allow"),
        }
    }

    #[test]
    fn caps_to_available_cash() {
        // can never spend more cash than is on hand, regardless of the configured order size
        let pf = PortfolioSnapshot {
            equity: 1000.0,
            daily_pnl_pct: 0.0,
            open_positions: 0,
            cash_thb: 150.0,
            deployed_pct: 0.0,
            session_pnl_pct: 0.0,
        };
        match evaluate(&cfg(), &pf, 500.0) {
            RiskDecision::Allow { max_amount_quote } => assert_eq!(max_amount_quote, 150.0),
            _ => panic!("expected Allow"),
        }
    }

    #[test]
    fn blocks_when_position_slots_are_full() {
        let pf = PortfolioSnapshot {
            equity: 1000.0,
            daily_pnl_pct: 0.0,
            open_positions: 5,
            cash_thb: 1000.0,
            deployed_pct: 0.0,
            session_pnl_pct: 0.0,
        };
        assert!(matches!(
            evaluate(&cfg(), &pf, 100.0),
            RiskDecision::Block { .. }
        ));
    }
}
