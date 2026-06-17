-- Quorum schema — เก็บประวัติการตัดสินใจอย่างละเอียดเพื่อทำ report

CREATE TABLE IF NOT EXISTS decisions (
    id                    BIGSERIAL PRIMARY KEY,
    symbol                TEXT        NOT NULL,
    quote                 TEXT        NOT NULL,
    mode                  TEXT        NOT NULL,            -- paper | live | signal-only
    final_action          TEXT        NOT NULL,            -- BUY | SELL | HOLD (จาก judge)
    consensus_action      TEXT        NOT NULL,            -- มติรวมก่อน judge
    consensus_confidence  DOUBLE PRECISION NOT NULL,
    agreement             INTEGER     NOT NULL,            -- จำนวน agent ที่เห็นตรง
    voted                 INTEGER     NOT NULL,            -- จำนวน agent ที่ออกเสียง
    vetoed                BOOLEAN     NOT NULL DEFAULT FALSE,
    judge_engine          TEXT        NOT NULL,            -- ollama:... | rule-based
    judge_reasoning       TEXT        NOT NULL,
    last_price            DOUBLE PRECISION,
    executed              BOOLEAN     NOT NULL DEFAULT FALSE,
    note                  TEXT        NOT NULL DEFAULT '',
    raw_analysis          JSONB,                           -- payload เต็มจาก AI (votes รายตัว ฯลฯ)
    created_at            TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS idx_decisions_symbol     ON decisions (symbol);
CREATE INDEX IF NOT EXISTS idx_decisions_created_at ON decisions (created_at DESC);
CREATE INDEX IF NOT EXISTS idx_decisions_action     ON decisions (final_action);

-- credential ของ broker (ใส่ผ่าน modal บนเว็บ)
-- หมายเหตุความปลอดภัย: production ควรเข้ารหัสด้วย pgcrypto/KMS
-- และ backend ไม่เคยส่ง api_secret กลับออกไปทาง API
CREATE TABLE IF NOT EXISTS broker_credentials (
    broker      TEXT PRIMARY KEY,                          -- "bitkub"
    api_key     TEXT NOT NULL,
    api_secret  TEXT NOT NULL,
    updated_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);
