-- Watchlist กลางของ local workspace
-- ใช้ให้ client refresh / backend restart แล้วยังจำชุดสินทรัพย์ที่ต้องเฝ้าได้

CREATE TABLE IF NOT EXISTS watch_symbols (
    symbol      TEXT PRIMARY KEY,
    sort_order  INT NOT NULL DEFAULT 0,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS idx_watch_symbols_order ON watch_symbols (sort_order, symbol);
