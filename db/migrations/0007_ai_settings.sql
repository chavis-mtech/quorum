-- Quorum 0007 — AI judge provider settings (Phase 5)
-- เก็บเฉพาะ preference ต่อบัญชี; API key ของ cloud provider ยังอยู่ใน broker_credentials ต่อ user

ALTER TABLE account_settings
    ADD COLUMN IF NOT EXISTS ai_judge_enabled BOOLEAN NOT NULL DEFAULT TRUE,
    ADD COLUMN IF NOT EXISTS ai_judge_provider TEXT NOT NULL DEFAULT 'ollama',
    ADD COLUMN IF NOT EXISTS ai_judge_model TEXT NOT NULL DEFAULT 'qwen3:14b',
    ADD COLUMN IF NOT EXISTS ai_judge_ollama_url TEXT NOT NULL DEFAULT 'http://localhost:11434',
    ADD COLUMN IF NOT EXISTS ai_judge_base_url TEXT NOT NULL DEFAULT '',
    ADD COLUMN IF NOT EXISTS ai_judge_thinking BOOLEAN NOT NULL DEFAULT TRUE;
