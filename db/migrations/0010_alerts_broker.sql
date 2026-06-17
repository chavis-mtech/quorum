-- 0010: ระบบ alert ต่อบัญชี + เตรียม multi-broker
--
-- alerts: เหตุการณ์ที่ user ควรรู้ (เงินไม่พอ, คำสั่งล้มเหลว, แผนถูกยกเลิก, rescue plan)
--   level: info | warn | error
--   code : รหัสเครื่อง (insufficient_funds, order_failed, plan_cancelled, ...) ใช้ filter/dedupe
CREATE TABLE IF NOT EXISTS alerts (
    id          BIGSERIAL PRIMARY KEY,
    account_id  BIGINT NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    level       TEXT NOT NULL DEFAULT 'info',
    code        TEXT NOT NULL DEFAULT '',
    message     TEXT NOT NULL,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX IF NOT EXISTS idx_alerts_account_time ON alerts(account_id, created_at DESC);

-- broker ต่อบัญชี: ตอนนี้ bitkub, อนาคต binance ฯลฯ (resolver เลือก adapter ตามค่านี้)
ALTER TABLE account_settings
    ADD COLUMN IF NOT EXISTS broker TEXT NOT NULL DEFAULT 'bitkub';
