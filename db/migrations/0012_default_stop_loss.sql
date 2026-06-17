-- 0012: Always-on hard stop-loss (Balanced risk profile)
--
-- Previously stop_loss_pct defaulted to 0.0 (= "no automatic stop"), so a position with no
-- plan-level stop could bleed indefinitely until the AI happened to decide to sell. That is the
-- main reason positions "waited and never recovered, then sold at a big loss".
--
-- Give every account a real 5% hard stop unless the user explicitly set a tighter one (>0).
-- This is independent of, and complementary to, the code-level catastrophic cap (MAX_LOSS_PCT)
-- which guarantees no single trade ever loses more than ~6% even if a plan's own stop is wider.
UPDATE account_settings
   SET stop_loss_pct = 0.05
 WHERE stop_loss_pct <= 0;

-- Make 5% the default for any newly-created accounts too (was 0.0).
ALTER TABLE account_settings
    ALTER COLUMN stop_loss_pct SET DEFAULT 0.05;
