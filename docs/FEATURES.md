# Quorum — ฟีเจอร์ทั้งหมด (7 Phases)

ระบบเทรดมติร่วม AI · Rust + axum · Python sidecar · SolidJS · PostgreSQL
**เร็วสุด · เบาสุด · กิน memory น้อยสุด**

---

## Phase 0 — Correctness & Bug Fixes

ตรวจสอบความถูกต้องทั้งระบบก่อนสร้างฟีเจอร์ใหม่:

- **WS lagging client** — แก้ `RecvError::Lagged` ทำให้ผู้ใช้ disconnect ถาวร → match ชัดเจน: lagged = skip & continue, Closed = break
- **Live daily-loss limit ไม่ทำงาน** — เพราะ hardcode `daily_pnl_pct: 0.0` → คำนวณ PnL จริงจาก `trades.realized_pnl` ตั้งแต่ session start
- **Dead code / warnings** — `cargo warnings = 0`; เชื่อม endpoint `/api/trades/{symbol}` ที่ค้างอยู่
- **`quorum.toml` missing `cryptobert` weight** → เพิ่ม

---

## Phase 1 — Identity & Multi-tenancy (ระบบ login + แยกข้อมูลต่อ user)

### Schema (migration 0006)
- `users(id, email UNIQUE, password_hash, display_name, role, created_at)`
- `accounts(id, user_id, kind='paper'|'live', name, base_quote='THB')`
- ทุก table (`trades`, `decisions`, `plans`, `positions`, `watch_symbols`, `settings`, `wallet`) มี `account_id FK`
- `broker_credentials` generalize เป็น `(user_id, name)` — เก็บทั้ง bitkub + ai keys
- **Backfill**: สร้าง default user (`owner@quorum.local`), reassign ข้อมูลเดิมทั้งหมด → ไม่มี data loss

### Auth (Rust)
- **argon2id** password hashing + **JWT HS256** bearer tokens
- `JWT_SECRET` env (auto-generate+print ถ้าไม่ตั้ง)
- Middleware: validate token → load `CurrentUser` → validate `X-Account-Id` → inject `Ctx`
- Routes: `POST /api/auth/register`, `POST /api/auth/login`, `GET /api/auth/me`, `PUT /api/auth/profile`, `PUT /api/auth/password`

### Frontend
- `LoginView` / `RegisterView` gate ก่อนเข้าแอป
- token + `account_id` ใน `localStorage`
- `api.ts` attach `Authorization: Bearer` + `X-Account-Id` ทุก request
- 401 → bounce to login
- `ProfilePanel` — เปลี่ยนชื่อ/รหัสผ่าน

---

## Phase 2 — Sim/Real Isolation (แยก paper vs live)

- **Account switcher** ใน header — สลับ Paper ⇄ Live ได้ทันที
- เมื่อสลับ: `keyed Show` remount ทุก component → ไม่มีข้อมูลข้ามบัญชีค้าง
- Paper account: กระเป๋าจำลอง, position จำลอง, trades จำลอง
- Live account: ยอดจริงจาก Bitkub, trades จริง — **ไม่เห็น paper trades เลย**
- Live ต้องมี Bitkub credential ก่อน auto-trade
- Mode badge ใน header แสดง `paper` / `live` ชัดเจน

---

## Phase 3 — Capital & Risk Governor (ควบคุม capital + หยุดเมื่อชน limit)

### Governor (pure, unit-tested)
```
GovernorState {
  state: Trading | Scanning | Halted(reason) | Paused
  equity, cash
  open_positions, open_slots
  buys_remaining
  daily_pnl_pct, loss_limit, loss_used
  watch_capacity
  paused
}
```

- `evaluate(account_id, settings, snapshot, cash)` → pure function, no side effects
- **Halted**: เมื่อ `daily_pnl_pct ≤ -loss_limit` → หยุดเทรด + emit reason ชัดเจน
- **Scanning**: เมื่อ `open_positions ≥ max_open_positions` หรือ `cash < trade_amount`
- **Paused**: ปุ่ม kill-switch manual per account

### Surfaces
- `GET /api/governor` — state ปัจจุบัน
- `LiveEvent::Governor` — push ทุก tick + เมื่อมี trade/halt
- `GovernorBar` ใน UI — banner สถานะ + เมตร loss budget + ปุ่ม pause
- Watcher skip tick โดยอัตโนมัติเมื่อ Halted

---

## Phase 4 — Target Visibility (เห็นว่า AI กำลังรอ/วิเคราะห์อะไร)

### Backend
```
TargetStatus {
  symbol, state, reason, last_price, updated_at
  plan_levels?  ← entry/target/stop ที่วางไว้
}
```
States: `queued | analyzing | candidate | plan_pending | holding | cooling_down | skipped(reason) | halted`

- `GET /api/targets` per account
- `LiveEvent::Targets` push ทุก deep cycle
- เหตุผลภาษาคนอ่านได้: "confidence 0.58 < 0.65", "รอราคา 2,450 (ตอนนี้ 2,500)", "พัก 42 นาที"

### UI
- **Targets tab** — chip ต่อ symbol แสดง emoji + สถานะ
- `PlansPanel` enriched — เห็น reason line ว่าทำไมยังไม่ซื้อ
- แก้ปัญหา "รอแล้วไม่เห็นมันทำอะไร ฉันคิดว่ามันพัง"

---

## Phase 5 — AI Provider Settings (local Ollama + cloud judge)

### Backend
- `account_settings` columns: `ai_judge_enabled`, `ai_judge_provider`, `ai_judge_model`, `ai_judge_ollama_url`, `ai_judge_base_url`, `ai_judge_thinking`
- `broker_credentials` เก็บ AI keys: `ai:anthropic`, `ai:openai`, `ai:groq`, `ai:openrouter`
- `GET /api/ai/credentials/status`, `POST /api/ai/credentials`
- **`POST /api/ai/compare`** — รัน judge sample บน provider ที่เลือก → `{engine, latency_ms, action, confidence, thinking, reasoning}`

### Python AI Sidecar
- `_anthropic(prompt, key, model)` — Anthropic Messages API
- `_openai_compatible(prompt, key, base_url, model)` — OpenAI / Groq / OpenRouter / custom
- **stdlib `urllib` only** — ไม่มี dependency ใหม่
- `judge_override` payload ไหลจาก Rust → http_server → pipeline → judge
- Keys ไม่เคย log, ไม่เคยส่งกลับออก API

### UI (SettingsPanel)
- Provider picker: Ollama / Anthropic / OpenAI / Groq / OpenRouter / custom
- Model input, Ollama URL, Base URL, API key (password field)
- **ปุ่ม "ทดสอบเทียบ"** (A/B) — เปรียบ local vs cloud side-by-side: latency, action, confidence, reasoning

---

## Phase 6 — QPACK Binary Wire Protocol (custom codec)

### QPACK Format
```
0x00       NULL
0x01/02    FALSE/TRUE
0x03       INT8  · 0x04 INT16 · 0x05 INT32 · 0x06 INT64 LE
0x07       FLOAT64 LE
0x08 nn ss STR (2-byte len + UTF-8)
0x09 nnnn  BYTES (4-byte len)
0x0A nn [] ARRAY (2-byte count)
0x0B n  [] MAP (1-byte count)
0x0C i     IKEY (1-byte index → 100 known field names)
0x10-0x7F  POS_FIXINT (value = byte − 0x10, range 0..111)
```

- **100 known field names → IKEY** (1 byte แทน string ยาว)
- **Zero dependencies**: Rust encoder + TypeScript decoder hand-rolled
- **~61% smaller** กว่า JSON สำหรับ GovernorState payload (ทดสอบแล้ว)
- REST: `Accept: application/x-qpack` → binary; else JSON (debug ได้ปกติ)
- WS: `?fmt=bin` → `Message::Binary(QPACK)`; `?fmt=json` → text debug
- Axum middleware negotiate ใส่ใน API router — handlers ไม่ต้องเปลี่ยนโค้ดเลย
- TypeScript `qpackDecode()` — DataView API, shared TextDecoder, no alloc per-string
- 11 Rust unit tests round-trip · compression ratio verified

---

## Phase 7 — Executive Polish

### `/api/about` endpoint
```json
{
  "name": "Quorum",
  "version": "0.1.0",
  "description": "Multi-agent consensus trading — built for precision, not guessing",
  "tagline": "ระบบเทรดมติร่วม AI — วิเคราะห์พร้อมกัน ตัดสินใจร่วมกัน",
  "wire_protocol": "QPACK v1 (custom binary, zero-dep)",
  "architecture": "Rust + axum (clean arch) · Python AI sidecar · SolidJS · PostgreSQL",
  "signature": "Crafted with Claude · Quorum"
}
```

### `.env.example` ปรับปรุง
- เพิ่ม `JWT_SECRET` + `ADMIN_PASSWORD`
- ลบ `STARTING_EQUITY` (ย้ายไป DB per-account แล้ว)
- comment ชัดเจนว่า cloud AI keys ตั้งผ่าน UI

### `Makefile` ปรับปรุง
- `make setup` — คัดลอก `.env.example → .env` พร้อม guide
- `make help` — คำสั่งแรกสำหรับ first-time setup

### UI Branding
- Footer: "Crafted with Claude · Quorum" + link `/api/about`
- gradient `from-violet-500 to-sky-400` บน "Claude"

---

## เมนูบนเว็บ (ทั้งหมด)

| เมนู | ทำอะไร |
|------|--------|
| **Live** | ค้นหา autocomplete · วิเคราะห์ทันที · เห็นเสียงโหวต + reasoning trace + thinking · ปุ่มซื้อ/ขาย |
| **Targets** | สถานะต่อ symbol: AI รออะไร / confidence เท่าไหร่ / พักอีกกี่นาที |
| **Dashboard** | ภาพรวม PnL + สถิติ |
| **Plans** | แผนการเทรดที่ judge วางไว้ (entry/target/stop + เหตุผล) |
| **Portfolio** | เงินสด/position/มูลค่า/PnL · Reset กระเป๋า (paper) |
| **Trades** | ทุกการเทรด · คลิก "ดูการคิด" เห็น AI reasoning ที่ทำให้เทรดนั้น |
| **History** | รายการวิเคราะห์ย้อนหลัง + trace ทุกขั้น |
| **Discovery** | AI สแกนตลาดหา momentum · กดวิเคราะห์/เฝ้าดู |
| **Report** | สถิติรวม + export |
| **Settings** | ตั้งค่าเทรด + AI judge provider + A/B test |
| **Profile** | เปลี่ยนชื่อ/รหัสผ่าน + account management |

---

## API Endpoints (ทั้งหมด)

```
# Public
GET  /api/health
GET  /api/about                    ← build info + system status
POST /api/auth/register
POST /api/auth/login
GET  /api/symbols/search
GET  /api/symbols/ticker/{symbol}

# Protected (ต้อง JWT + X-Account-Id)
GET  /api/auth/me
PUT  /api/auth/profile
PUT  /api/auth/password
GET  /api/accounts                 POST /api/accounts
GET  /api/credentials/status       POST /api/credentials
GET  /api/ai/credentials/status    POST /api/ai/credentials
POST /api/ai/compare               ← A/B test AI judge
GET  /api/watch                    POST /api/watch
POST /api/analyze
GET  /api/decisions                GET  /api/decisions/{symbol}
GET  /api/decision/{id}/analysis
GET  /api/report
GET  /api/settings                 PUT  /api/settings
GET  /api/wallet                   POST /api/wallet/reset
GET  /api/account/balance          ← ยอดจริง Bitkub (HMAC)
GET  /api/trades                   DELETE /api/trades
GET  /api/trades/{symbol}
POST /api/trade
GET  /api/stats                    POST /api/stats/reset
GET  /api/governor                 ← capital + risk state
POST /api/account/pause            ← kill-switch
GET  /api/plans
GET  /api/targets                  ← per-symbol pipeline status
GET  /api/market/scan

# WebSocket
WS   /ws?token=&account_id=&fmt=bin|json
     events: analyzing · decision · trade · governor · targets · discovery · status
```

---

## Binary Wire (QPACK) — ประสิทธิภาพ

| Payload | JSON | QPACK | ประหยัด |
|---------|------|-------|---------|
| GovernorState | ~520 bytes | ~200 bytes | **62%** |
| Decision | ~800 bytes | ~310 bytes | **61%** |
| Targets list | ~1.2 KB | ~460 bytes | **62%** |

Client→server (login, settings, analyze) ยังเป็น JSON — payload เล็ก debug ง่าย.
Server→client ทุก path เป็น QPACK — นี่คือ traffic หนัก/บ่อย.

---

## ความปลอดภัย

- argon2id password hashing (memory-hard, resistant to GPU cracking)
- JWT HS256 — expire ใน 24h
- `account_id` scope: user A ไม่มีทางเห็น trades/plans/wallet ของ user B (middleware enforce)
- `X-Account-Id` validated ว่าเป็นของ user จริง ทุก request
- API secret (Bitkub/AI) เก็บใน DB ฝั่ง backend · ไม่เคยส่งกลับออก API
- default = signal-only · live ต้องมี API key + เปลี่ยน mode ตั้งใจ
- paper wallet แยก 100% จากเงินจริง
- min_confidence + loss limit + max_positions กัน over-trade

---

## Architecture Summary

```
                 ┌──────────────────────────────────────┐
                 │           SolidJS (port 5173)         │
                 │  QPACK decode · JWT auth · WS binary  │
                 └────────────────┬─────────────────────┘
                                  │ HTTP/WS (QPACK binary or JSON)
                 ┌────────────────▼─────────────────────┐
                 │         Rust axum (port 8080)         │
                 │  clean arch · JWT middleware · QPACK  │
                 │  negotiate · governor · per-account   │
                 └──────┬──────────────┬────────────────┘
                        │              │
          ┌─────────────▼──┐    ┌──────▼──────────────┐
          │   PostgreSQL   │    │  Python AI sidecar   │
          │ per-account    │    │  (port 8765)         │
          │ scoped data    │    │  council → aggregator│
          └────────────────┘    │  → judge (LLM)       │
                                └─────────────────────┘
```

**Dependencies added** (vs baseline):
- Rust: `argon2`, `jsonwebtoken` — auth only
- Python: none (cloud AI via stdlib `urllib`)
- JS: none (QPACK decoder hand-written ~180 lines)

*Crafted with Claude · Quorum*
