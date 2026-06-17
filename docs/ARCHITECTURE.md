# สถาปัตยกรรม Quorum (Clean Architecture)

Quorum จัดตาม **Clean / Hexagonal Architecture** — แยกเป็นชั้นที่พึ่งพาเข้าด้านในเสมอ
(`presentation → application → domain`, ส่วน `infrastructure` implement interface ที่ domain กำหนด)

```
┌──────────────────────────────────────────────────────────────┐
│  Browser — SolidJS + TailwindCSS                              │
│  Dashboard · WatchList · AgentVotes · Consensus · Report      │
│  CredentialsModal (เด้งใส่ API key ตอนเปิด)                    │
└───────────────┬───────────────────────────┬───────────────────┘
        HTTP /api                      WebSocket /ws (LiveEvent)
┌───────────────▼───────────────────────────▼───────────────────┐
│  Rust Backend (axum)                                           │
│                                                                │
│  presentation/   handlers · ws · routes · AppState            │
│        │ เรียก use case ผ่าน trait เท่านั้น                    │
│  application/    TradingService · Watcher · risk rules        │
│        │ พึ่งพา "ports" (trait) ไม่รู้จัก DB/HTTP จริง         │
│  domain/         models (Action, Analysis, DecisionRecord...)  │
│                  ports (AiEngine, Broker, HistoryRepository,   │
│                         SecretStore, EventSink)               │
│        ▲ infrastructure implement ports เหล่านี้               │
│  infrastructure/ ai_sidecar(HTTP) · bitkub · postgres(sqlx)   │
│                  events(broadcast)                            │
└──────┬─────────────────────────┬──────────────────┬───────────┘
   HTTP│                  sqlx   │                   │ HTTP
┌──────▼───────────┐   ┌─────────▼────────┐   ┌──────▼───────────┐
│ Python AI layer  │   │   PostgreSQL     │   │   Ollama (LLM)   │
│ agents·aggregator│   │ decisions (JSONB)│   │  Judge ชี้ขาด     │
│ ·judge·pipeline  │   │ broker_credential│   │                  │
└──────────────────┘   └──────────────────┘   └──────────────────┘
```

## ทำไมแยกแบบนี้

| ชั้น | รับผิดชอบ | กฎ |
|------|-----------|-----|
| **domain** | entities + interface (ports) | ไม่ import อะไรจากชั้นนอกเลย — ทดสอบล้วนได้ |
| **application** | use case (วิเคราะห์→risk→เทรด→บันทึก) | รู้จักแค่ `domain` + ports |
| **infrastructure** | ต่อ DB/broker/AI/WS จริง | implement ports ของ domain |
| **presentation** | HTTP/WebSocket (axum) | แปลง request → เรียก use case |

ผลที่ได้: สลับ PostgreSQL เป็นอย่างอื่น, เปลี่ยน Python sidecar เป็น gRPC,
หรือเพิ่ม broker ใหม่ — แก้แค่ `infrastructure` โดยไม่แตะ business logic

## Flow การตัดสินใจ (ตาม flow เดิม)

```
Watcher (ทุก N วินาที ต่อ symbol)
   └─> TradingService.run_once(symbol)
        1. AiEngine.analyze(symbol)            → Python: agents วิเคราะห์พร้อมกัน
             technical · trend_ml · finbert · news
             → aggregator (weighted vote + threshold + veto)
             → judge (Ollama) ชี้ขาด
        2. risk.evaluate(...)                  → Rust: override/block ได้
        3. broker.place_order(...)             → paper/bitkub (ตาม mode)
        4. HistoryRepository.save_decision(...) → PostgreSQL (เก็บ JSONB เต็ม)
        5. EventSink.publish(LiveEvent)        → WebSocket → UI update real-time
```

## โมเดลข้อมูลหลัก (domain)

- `Analysis` — ผลเต็มจาก AI (consensus + votes รายตัว + verdict)
- `DecisionRecord` — สิ่งที่ persist ลง DB (ใช้ทำ report)
- `LiveEvent` — `Analyzing | Decision | Status` ที่ push ไป UI
- `RiskDecision` — `Allow{max_amount} | Block{reason}`

ดู [`backend/src/domain/`](../backend/src/domain/) เป็นจุดเริ่มอ่านโค้ด
