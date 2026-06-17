-- Quorum — แผนเทรด (trade plans): AI วาง thesis + จุดเข้า/เป้า/ตัดขาดทุน แล้วติดตาม
-- หนึ่งแผน active ต่อ 1 สินทรัพย์ (upsert ด้วย symbol)

CREATE TABLE IF NOT EXISTS trade_plans (
    id           BIGSERIAL PRIMARY KEY,
    symbol       TEXT        NOT NULL UNIQUE,         -- active plan ต่อเหรียญ
    quote        TEXT        NOT NULL DEFAULT 'THB',
    state        TEXT        NOT NULL DEFAULT 'pending', -- pending|open|closed|cancelled
    action       TEXT        NOT NULL,                -- BUY|SELL
    entry_type   TEXT        NOT NULL DEFAULT 'market',
    entry_price  DOUBLE PRECISION NOT NULL DEFAULT 0,
    target_price DOUBLE PRECISION NOT NULL DEFAULT 0,
    stop_price   DOUBLE PRECISION NOT NULL DEFAULT 0,
    confidence   DOUBLE PRECISION NOT NULL DEFAULT 0,
    thesis       TEXT        NOT NULL DEFAULT '',
    invalidation TEXT        NOT NULL DEFAULT '',
    next_step    TEXT        NOT NULL DEFAULT '',
    decision_id  BIGINT,
    last_price   DOUBLE PRECISION NOT NULL DEFAULT 0,
    created_at   TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at   TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS idx_plans_state ON trade_plans (state);
