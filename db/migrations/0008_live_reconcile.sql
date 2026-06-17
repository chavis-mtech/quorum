-- Quorum 0008 — live broker reconciliation
-- ให้ sync ประวัติ order จาก Bitkub แล้ว upsert กลับมาแก้ record ที่เคยบันทึก amount เป็น 0 ได้

CREATE UNIQUE INDEX IF NOT EXISTS idx_trades_account_external_order
    ON trades (account_id, external_order_id)
    WHERE external_order_id <> '';
