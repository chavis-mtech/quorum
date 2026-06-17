-- Quorum — ตารางสำหรับการเทรด, กระเป๋าจำลอง, ตั้งค่า, และการค้นหาตลาด

-- ====== การตั้งค่าการเทรด (แถวเดียว id=1) ======
CREATE TABLE IF NOT EXISTS trading_settings (
    id                  INT PRIMARY KEY DEFAULT 1,
    mode                TEXT        NOT NULL DEFAULT 'paper',     -- paper | live | signal-only
    auto_trade          BOOLEAN     NOT NULL DEFAULT FALSE,       -- ให้ AI ซื้อขายเองไหม
    trade_amount_quote  DOUBLE PRECISION NOT NULL DEFAULT 1000,  -- เงินต่อ 1 คำสั่ง (THB)
    max_position_pct    DOUBLE PRECISION NOT NULL DEFAULT 0.10,  -- เพดานต่อ position (ของพอร์ต)
    min_confidence      DOUBLE PRECISION NOT NULL DEFAULT 0.65,  -- เทรดเมื่อ judge มั่นใจ >= ค่านี้
    daily_loss_limit    DOUBLE PRECISION NOT NULL DEFAULT 0.05,  -- หยุดเทรดเมื่อขาดทุนเกิน/วัน
    max_open_positions  INT         NOT NULL DEFAULT 5,
    allow_sell          BOOLEAN     NOT NULL DEFAULT TRUE,        -- อนุญาตขายไหม (บางคนซื้ออย่างเดียว)
    take_profit_pct     DOUBLE PRECISION NOT NULL DEFAULT 0.0,   -- ขายทำกำไรอัตโนมัติ (0=ปิด)
    stop_loss_pct       DOUBLE PRECISION NOT NULL DEFAULT 0.0,   -- ตัดขาดทุนอัตโนมัติ (0=ปิด)
    discovery_enabled   BOOLEAN     NOT NULL DEFAULT FALSE,      -- ให้ AI สแกนหาตลาดเอง
    discovery_top_n     INT         NOT NULL DEFAULT 5,          -- สแกนแล้วเลือกกี่ตัว
    updated_at          TIMESTAMPTZ NOT NULL DEFAULT now()
);
INSERT INTO trading_settings (id) VALUES (1) ON CONFLICT (id) DO NOTHING;

-- ====== กระเป๋าจำลอง (paper) — แถวเดียว id=1 ======
CREATE TABLE IF NOT EXISTS paper_wallet (
    id            INT PRIMARY KEY DEFAULT 1,
    cash_quote    DOUBLE PRECISION NOT NULL DEFAULT 100000,   -- เงินสดคงเหลือ (THB)
    starting_cash DOUBLE PRECISION NOT NULL DEFAULT 100000,
    updated_at    TIMESTAMPTZ NOT NULL DEFAULT now()
);
INSERT INTO paper_wallet (id) VALUES (1) ON CONFLICT (id) DO NOTHING;

-- positions ในกระเป๋าจำลอง
CREATE TABLE IF NOT EXISTS paper_positions (
    symbol      TEXT PRIMARY KEY,
    amount_base DOUBLE PRECISION NOT NULL DEFAULT 0,   -- จำนวนเหรียญที่ถือ
    avg_price   DOUBLE PRECISION NOT NULL DEFAULT 0,   -- ราคาทุนเฉลี่ย
    updated_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- ====== ประวัติการเทรด (ผูกกับการวิเคราะห์ผ่าน decision_id) ======
CREATE TABLE IF NOT EXISTS trades (
    id                BIGSERIAL PRIMARY KEY,
    decision_id       BIGINT REFERENCES decisions(id) ON DELETE SET NULL,  -- ผูกกับการวิเคราะห์
    symbol            TEXT        NOT NULL,
    quote             TEXT        NOT NULL,
    side              TEXT        NOT NULL,        -- BUY | SELL
    mode              TEXT        NOT NULL,        -- paper | live
    simulated         BOOLEAN     NOT NULL,
    amount_base       DOUBLE PRECISION NOT NULL,  -- จำนวนเหรียญ
    amount_quote      DOUBLE PRECISION NOT NULL,  -- มูลค่า (THB)
    price             DOUBLE PRECISION NOT NULL,
    status            TEXT        NOT NULL,        -- filled | failed | pending
    external_order_id TEXT        NOT NULL DEFAULT '',
    note              TEXT        NOT NULL DEFAULT '',
    created_at        TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX IF NOT EXISTS idx_trades_symbol     ON trades (symbol);
CREATE INDEX IF NOT EXISTS idx_trades_created_at ON trades (created_at DESC);
CREATE INDEX IF NOT EXISTS idx_trades_decision   ON trades (decision_id);

-- ====== ผลการสแกนตลาด (AI ค้นหาเอง) ======
CREATE TABLE IF NOT EXISTS market_scans (
    id          BIGSERIAL PRIMARY KEY,
    symbol      TEXT        NOT NULL,
    score       DOUBLE PRECISION NOT NULL,   -- คะแนนน่าสนใจ
    reason      TEXT        NOT NULL DEFAULT '',
    last_price  DOUBLE PRECISION,
    change_24h  DOUBLE PRECISION,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX IF NOT EXISTS idx_market_scans_created ON market_scans (created_at DESC);
