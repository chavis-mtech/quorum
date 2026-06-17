-- Quorum 0006 — Identity & multi-tenancy
-- เปลี่ยนจากเครื่องมือผู้ใช้คนเดียว (global singletons) → ระบบหลายผู้ใช้
-- ทุก user แยกขาดกัน, แต่ละ user มี "บัญชี" (account) คนละใบ: paper + live
-- ข้อมูลเดิมทั้งหมดถูกยกให้ default user (id=1) บัญชี paper (id=1) แบบไม่สูญหาย

-- ====== users ======
CREATE TABLE IF NOT EXISTS users (
    id            BIGSERIAL PRIMARY KEY,
    email         TEXT        NOT NULL UNIQUE,
    password_hash TEXT        NOT NULL,           -- argon2 (ตั้งจริงโดย backend)
    display_name  TEXT        NOT NULL DEFAULT '',
    role          TEXT        NOT NULL DEFAULT 'user',  -- 'user' | 'admin'
    created_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at    TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- ====== accounts (บัญชีเทรดของแต่ละ user) ======
CREATE TABLE IF NOT EXISTS accounts (
    id          BIGSERIAL PRIMARY KEY,
    user_id     BIGINT      NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    kind        TEXT        NOT NULL,             -- 'paper' | 'live'
    name        TEXT        NOT NULL DEFAULT '',
    base_quote  TEXT        NOT NULL DEFAULT 'THB',
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (user_id, name)
);
CREATE INDEX IF NOT EXISTS idx_accounts_user ON accounts (user_id);

-- ====== default user + บัญชี (ยกข้อมูลเดิมมาให้) ======
-- password_hash = 'SETME' → backend จะตั้งรหัสจริงจาก env ตอน boot ครั้งแรก
INSERT INTO users (id, email, password_hash, display_name, role)
    VALUES (1, 'owner@quorum.local', 'SETME', 'Owner', 'admin')
    ON CONFLICT DO NOTHING;
SELECT setval(pg_get_serial_sequence('users','id'), GREATEST((SELECT MAX(id) FROM users), 1));

INSERT INTO accounts (id, user_id, kind, name) VALUES
    (1, 1, 'paper', 'Paper'),
    (2, 1, 'live',  'Live')
    ON CONFLICT DO NOTHING;
SELECT setval(pg_get_serial_sequence('accounts','id'), GREATEST((SELECT MAX(id) FROM accounts), 1));

-- ====== การตั้งค่าเทรด: ต่อบัญชี (แทน trading_settings แถวเดียว) ======
CREATE TABLE IF NOT EXISTS account_settings (
    account_id          BIGINT PRIMARY KEY REFERENCES accounts(id) ON DELETE CASCADE,
    mode                TEXT        NOT NULL DEFAULT 'paper',
    auto_trade          BOOLEAN     NOT NULL DEFAULT FALSE,
    trade_amount_quote  DOUBLE PRECISION NOT NULL DEFAULT 1000,
    max_position_pct    DOUBLE PRECISION NOT NULL DEFAULT 0.10,
    min_confidence      DOUBLE PRECISION NOT NULL DEFAULT 0.65,
    daily_loss_limit    DOUBLE PRECISION NOT NULL DEFAULT 0.05,
    max_open_positions  INT         NOT NULL DEFAULT 5,
    allow_sell          BOOLEAN     NOT NULL DEFAULT TRUE,
    take_profit_pct     DOUBLE PRECISION NOT NULL DEFAULT 0.0,
    stop_loss_pct       DOUBLE PRECISION NOT NULL DEFAULT 0.0,
    discovery_enabled   BOOLEAN     NOT NULL DEFAULT FALSE,
    discovery_top_n     INT         NOT NULL DEFAULT 5,
    paused              BOOLEAN     NOT NULL DEFAULT FALSE,   -- kill-switch (Phase 3)
    updated_at          TIMESTAMPTZ NOT NULL DEFAULT now()
);
-- ยกค่าตั้งเดิม (id=1) → บัญชี paper (account 1)
INSERT INTO account_settings
    (account_id, mode, auto_trade, trade_amount_quote, max_position_pct, min_confidence,
     daily_loss_limit, max_open_positions, allow_sell, take_profit_pct, stop_loss_pct,
     discovery_enabled, discovery_top_n)
SELECT 1, mode, auto_trade, trade_amount_quote, max_position_pct, min_confidence,
       daily_loss_limit, max_open_positions, allow_sell, take_profit_pct, stop_loss_pct,
       discovery_enabled, discovery_top_n
FROM trading_settings WHERE id = 1
ON CONFLICT DO NOTHING;
-- บัญชี live (account 2): เริ่มที่ signal-only เพื่อความปลอดภัย (ไม่แตะเงินจริงจนกว่าจะเปิดเอง)
INSERT INTO account_settings (account_id, mode) VALUES (2, 'signal-only')
    ON CONFLICT DO NOTHING;

-- ====== กระเป๋าจำลอง: ต่อบัญชี (แทน paper_wallet แถวเดียว) ======
CREATE TABLE IF NOT EXISTS account_wallet (
    account_id    BIGINT PRIMARY KEY REFERENCES accounts(id) ON DELETE CASCADE,
    cash_quote    DOUBLE PRECISION NOT NULL DEFAULT 100000,
    starting_cash DOUBLE PRECISION NOT NULL DEFAULT 100000,
    session_start TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at    TIMESTAMPTZ NOT NULL DEFAULT now()
);
INSERT INTO account_wallet (account_id, cash_quote, starting_cash, session_start)
SELECT 1, cash_quote, starting_cash, session_start FROM paper_wallet WHERE id = 1
ON CONFLICT DO NOTHING;

-- เลิกใช้ singleton เดิม (คัดลอกค่าครบแล้ว)
DROP TABLE IF EXISTS trading_settings;
DROP TABLE IF EXISTS paper_wallet;

-- ====== scope ตารางที่เหลือด้วย account_id ======

-- positions: PK(symbol) → PK(account_id, symbol)
ALTER TABLE paper_positions ADD COLUMN IF NOT EXISTS account_id BIGINT REFERENCES accounts(id) ON DELETE CASCADE;
UPDATE paper_positions SET account_id = 1 WHERE account_id IS NULL;
ALTER TABLE paper_positions ALTER COLUMN account_id SET NOT NULL;
ALTER TABLE paper_positions DROP CONSTRAINT IF EXISTS paper_positions_pkey;
ALTER TABLE paper_positions ADD PRIMARY KEY (account_id, symbol);

-- trades
ALTER TABLE trades ADD COLUMN IF NOT EXISTS account_id BIGINT REFERENCES accounts(id) ON DELETE CASCADE;
UPDATE trades SET account_id = 1 WHERE account_id IS NULL;
ALTER TABLE trades ALTER COLUMN account_id SET NOT NULL;
CREATE INDEX IF NOT EXISTS idx_trades_account ON trades (account_id);

-- decisions (ประวัติการวิเคราะห์ — แยกต่อบัญชี)
ALTER TABLE decisions ADD COLUMN IF NOT EXISTS account_id BIGINT REFERENCES accounts(id) ON DELETE CASCADE;
UPDATE decisions SET account_id = 1 WHERE account_id IS NULL;
ALTER TABLE decisions ALTER COLUMN account_id SET NOT NULL;
CREATE INDEX IF NOT EXISTS idx_decisions_account ON decisions (account_id);

-- trade_plans: UNIQUE(symbol) → UNIQUE(account_id, symbol)
ALTER TABLE trade_plans ADD COLUMN IF NOT EXISTS account_id BIGINT REFERENCES accounts(id) ON DELETE CASCADE;
UPDATE trade_plans SET account_id = 1 WHERE account_id IS NULL;
ALTER TABLE trade_plans ALTER COLUMN account_id SET NOT NULL;
ALTER TABLE trade_plans DROP CONSTRAINT IF EXISTS trade_plans_symbol_key;
CREATE UNIQUE INDEX IF NOT EXISTS trade_plans_acct_sym ON trade_plans (account_id, symbol);

-- watch_symbols: PK(symbol) → PK(account_id, symbol)
ALTER TABLE watch_symbols ADD COLUMN IF NOT EXISTS account_id BIGINT REFERENCES accounts(id) ON DELETE CASCADE;
UPDATE watch_symbols SET account_id = 1 WHERE account_id IS NULL;
ALTER TABLE watch_symbols ALTER COLUMN account_id SET NOT NULL;
ALTER TABLE watch_symbols DROP CONSTRAINT IF EXISTS watch_symbols_pkey;
ALTER TABLE watch_symbols ADD PRIMARY KEY (account_id, symbol);

-- ====== broker_credentials: ต่อ user (เก็บ key ของ broker + ของ AI cloud) ======
ALTER TABLE broker_credentials ADD COLUMN IF NOT EXISTS user_id BIGINT REFERENCES users(id) ON DELETE CASCADE;
UPDATE broker_credentials SET user_id = 1 WHERE user_id IS NULL;
ALTER TABLE broker_credentials ALTER COLUMN user_id SET NOT NULL;
ALTER TABLE broker_credentials DROP CONSTRAINT IF EXISTS broker_credentials_pkey;
ALTER TABLE broker_credentials ADD PRIMARY KEY (user_id, broker);
