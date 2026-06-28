-- 0013: Tighten the hard stop-loss (Balanced risk profile, 5% -> 3.5%)
--
-- Live data (account 4, 73 real trades over 3 weeks) showed a 53% win rate but avg loss
-- -9.33 vs avg win +5.11 per trade — net negative expectancy despite the directional edge being
-- real. The worst losers (AAVE/AERO/ID/AXL) realized ~10.6%, almost double the intended 5-6% cap,
-- because thin-liquidity market sells slipped past the stop before the 60s watch loop caught up.
-- Tightening the normal stop (and the catastrophic cap in code, MAX_LOSS_PCT 0.06 -> 0.05) shrinks
-- the loss side of that asymmetry without touching the win side or signal direction.
UPDATE account_settings
   SET stop_loss_pct = 0.035
 WHERE stop_loss_pct = 0.05;

ALTER TABLE account_settings
    ALTER COLUMN stop_loss_pct SET DEFAULT 0.035;
