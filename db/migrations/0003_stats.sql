-- Quorum — กำไร/ขาดทุนที่เกิดจริง (realized P&L) + session การนับสถิติ

-- กำไร/ขาดทุนที่เกิดขึ้นจริงต่อการขาย 1 ครั้ง (paper) = (ราคาขาย - ทุนเฉลี่ย) × จำนวน
ALTER TABLE trades ADD COLUMN IF NOT EXISTS realized_pnl DOUBLE PRECISION NOT NULL DEFAULT 0;

-- จุดเริ่มนับสถิติของกระเป๋าจำลอง (reset เพื่อเริ่ม session ใหม่ตอนเปลี่ยนกลยุทธ์)
ALTER TABLE paper_wallet ADD COLUMN IF NOT EXISTS session_start TIMESTAMPTZ NOT NULL DEFAULT now();
