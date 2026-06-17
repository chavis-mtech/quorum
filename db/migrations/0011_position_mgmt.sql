-- 0011: Active position management — trailing stop / breakeven + per-account management style
--
-- trade_plans: track the data a "professional trader" needs to manage an open position over time
--   high_water_mark — peak price reached since entry (basis for the trailing stop)
--   initial_stop    — the stop set at entry; risk R = entry - initial_stop never changes (used for R-multiples)
--   trail_active    — true once breakeven/trailing has kicked in (UI badge + logic flag)
ALTER TABLE trade_plans
    ADD COLUMN IF NOT EXISTS high_water_mark DOUBLE PRECISION NOT NULL DEFAULT 0,
    ADD COLUMN IF NOT EXISTS initial_stop    DOUBLE PRECISION NOT NULL DEFAULT 0,
    ADD COLUMN IF NOT EXISTS trail_active    BOOLEAN          NOT NULL DEFAULT false;

-- account_settings: how aggressively to manage open positions
--   manage_style    — off | conservative | balanced | aggressive (controls breakeven/trail R-multiples in code)
--   let_winners_run — when true, hitting the target tightens the trail instead of a hard take-profit
ALTER TABLE account_settings
    ADD COLUMN IF NOT EXISTS manage_style    TEXT    NOT NULL DEFAULT 'conservative',
    ADD COLUMN IF NOT EXISTS let_winners_run BOOLEAN NOT NULL DEFAULT true;

-- backfill initial_stop for any already-open plans so existing positions get managed too
UPDATE trade_plans SET initial_stop = stop_price WHERE initial_stop = 0 AND stop_price > 0;
