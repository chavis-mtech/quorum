//! Preflight — validates orders before submission, rejecting those that will "definitely fail" at the source
//!
//! Core idea: if we already know the broker will reject (insufficient funds, below minimum, no price),
//! don't waste time/API quota sending it — notify the user via alert and record a log instead
//!
//! Pure logic, no IO → easy to test, covers all scenarios

/// Result of a buy order preflight check
#[derive(Debug, Clone, PartialEq)]
pub enum BuyPreflight {
    /// Passed — submit the order with this amount
    Proceed { amount_quote: f64 },
    /// Insufficient funds for full amount, but reduced size still exceeds minimum → submit shrunk + notify user
    Shrink { amount_quote: f64, reason: String },
    /// Submitting would definitely fail → do not submit + notify user
    Block { code: &'static str, reason: String },
}

/// Safety buffer: buying with exact full balance is often rejected due to fees/rounding
const CASH_SAFETY: f64 = 0.995;

/// Validates a "buy" order before submission
///
/// * `amount_quote`   — desired purchase amount (already passed through risk cap)
/// * `cash_available` — actual cash balance in account (THB)
/// * `min_order_quote` — broker minimum order size (Bitkub ~10 THB; 0 = unknown → use 10)
/// * `last_price`     — latest market price (≤0 = price unavailable)
pub fn check_buy(
    amount_quote: f64,
    cash_available: f64,
    min_order_quote: f64,
    last_price: f64,
) -> BuyPreflight {
    let min_order = if min_order_quote > 0.0 {
        min_order_quote
    } else {
        10.0
    };
    if last_price <= 0.0 {
        return BuyPreflight::Block {
            code: "no_price",
            reason: "Market price unavailable — order not submitted to prevent buying at a bad price".into(),
        };
    }
    if amount_quote <= 0.0 {
        return BuyPreflight::Block {
            code: "zero_amount",
            reason: "Order size is 0 (risk cap or configuration left no available budget)".into(),
        };
    }
    if amount_quote < min_order {
        return BuyPreflight::Block {
            code: "below_min",
            reason: format!(
                "Order size {amount_quote:.2} is below broker minimum {min_order:.2} — order not submitted"
            ),
        };
    }
    let spendable = cash_available * CASH_SAFETY;
    if spendable >= amount_quote {
        return BuyPreflight::Proceed { amount_quote };
    }
    // Insufficient funds for full amount — reduce to available cash if still above minimum, otherwise block
    if spendable >= min_order {
        BuyPreflight::Shrink {
            amount_quote: spendable,
            reason: format!(
                "Cash {cash_available:.2} insufficient for {amount_quote:.2} — reduced order size to {spendable:.2} to fit available balance"
            ),
        }
    } else {
        BuyPreflight::Block {
            code: "insufficient_funds",
            reason: format!(
                "Cash {cash_available:.2} insufficient (need {amount_quote:.2}, minimum {min_order:.2}) — order not submitted as purchase is impossible"
            ),
        }
    }
}

/// Validates a "sell" order before submission — checks if there is something to sell and price is readable
pub fn check_sell(amount_base_held: f64, last_price: f64) -> Result<(), (&'static str, String)> {
    if last_price <= 0.0 {
        return Err((
            "no_price",
            "Market price unavailable — sell order not submitted".into(),
        ));
    }
    if amount_base_held <= 0.0 {
        return Err((
            "nothing_to_sell",
            "No coins in account to sell — skipping order".into(),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn proceeds_when_cash_covers_order() {
        assert_eq!(
            check_buy(1000.0, 5000.0, 10.0, 100.0),
            BuyPreflight::Proceed {
                amount_quote: 1000.0
            }
        );
    }

    #[test]
    fn blocks_when_no_price() {
        assert!(matches!(
            check_buy(1000.0, 5000.0, 10.0, 0.0),
            BuyPreflight::Block { code: "no_price", .. }
        ));
    }

    #[test]
    fn blocks_zero_or_below_min_amount() {
        assert!(matches!(
            check_buy(0.0, 5000.0, 10.0, 100.0),
            BuyPreflight::Block { code: "zero_amount", .. }
        ));
        assert!(matches!(
            check_buy(5.0, 5000.0, 10.0, 100.0),
            BuyPreflight::Block { code: "below_min", .. }
        ));
    }

    #[test]
    fn shrinks_to_available_cash_when_still_above_min() {
        match check_buy(1000.0, 500.0, 10.0, 100.0) {
            BuyPreflight::Shrink { amount_quote, .. } => {
                assert!((amount_quote - 497.5).abs() < 1e-9); // 500 * 0.995
            }
            other => panic!("expected Shrink but got {other:?}"),
        }
    }

    #[test]
    fn blocks_when_cash_below_min_order() {
        // Only 8 THB remaining, minimum is 10 — would be rejected, so don't submit
        assert!(matches!(
            check_buy(1000.0, 8.0, 10.0, 100.0),
            BuyPreflight::Block { code: "insufficient_funds", .. }
        ));
    }

    #[test]
    fn unknown_min_falls_back_to_ten_baht() {
        // min_order_quote=0 (unknown) → use 10 THB as fallback
        assert!(matches!(
            check_buy(9.0, 5000.0, 0.0, 100.0),
            BuyPreflight::Block { code: "below_min", .. }
        ));
    }

    #[test]
    fn cash_exactly_at_order_uses_safety_margin() {
        // Exact balance match → safety margin reduces to 99.5% (guards against fees)
        match check_buy(1000.0, 1000.0, 10.0, 100.0) {
            BuyPreflight::Shrink { amount_quote, .. } => {
                assert!((amount_quote - 995.0).abs() < 1e-9);
            }
            other => panic!("expected Shrink but got {other:?}"),
        }
    }

    #[test]
    fn sell_requires_holding_and_price() {
        assert!(check_sell(1.5, 100.0).is_ok());
        assert_eq!(check_sell(0.0, 100.0).unwrap_err().0, "nothing_to_sell");
        assert_eq!(check_sell(1.5, 0.0).unwrap_err().0, "no_price");
    }
}
