# ⚖️ Quorum — Multi-Agent Consensus Trading (Web)

Trading bot ที่ **ไม่เชื่อ AI ตัวเดียว** — ให้โมเดล AI สำเร็จรูปหลายตัวที่มองคนละมุม
วิเคราะห์สินทรัพย์เดียวกัน **พร้อมกัน** แล้วโหวตหามติร่วม จากนั้น **Judge (LLM local)**
ชี้ขาด มี **ชั้นตรวจข่าวบน network** + **risk layer** คุมอีกชั้น — ทุกอย่างแสดงผ่าน
**เว็บ (SolidJS + TailwindCSS)** real-time และเก็บประวัติละเอียดใน **PostgreSQL** เพื่อทำ report

> ⚠️ เพื่อการศึกษา การเทรดมีความเสี่ยงสูง อาจสูญเงินทั้งหมด AI ทำนายผิดได้เสมอ
> ไม่ใช่คำแนะนำการลงทุน เริ่มที่ `signal-only`/`paper` เสมอ

---

## ภาพรวมระบบ

```
Browser (SolidJS + Tailwind)  ──HTTP /api──▶  Rust backend (axum, clean arch)
   live dashboard · report           ──WS /ws──▶     │
   credentials modal                                  ├─▶ Python AI (agents+judge)
                                                       ├─▶ PostgreSQL (history)
                                                       └─▶ Ollama (Judge LLM)
```

- **Frontend (เว็บ):** หน้า Live บอก *"ตอนนี้กำลังวิเคราะห์ตัวไหน ตัดสินใจอย่างไร"*,
  เลือกหุ้น/เหรียญเองได้, แท็บ Report สรุปสถิติ + ประวัติเต็ม, **modal เด้งใส่ Bitkub API key ตอนเปิด**
- **Backend (Rust):** Clean Architecture (domain / application / infrastructure / presentation)
- **AI layer (Python):** คณะที่ปรึกษา 4 ตัว (technical · trend_ml · finbert · news) → aggregator → judge
- **PostgreSQL:** เก็บทุกการตัดสินใจ + payload เต็ม (JSONB) ไว้ทำ report
- **Ollama:** Judge LLM แบบ local (แนะนำ `qwen3:14b` สำหรับ Mac 24GB)

## เอกสาร

| ไฟล์ | เนื้อหา |
|------|---------|
| [docs/BUILD.md](docs/BUILD.md) | ติดตั้ง + build ทั้ง 3 ส่วน |
| [docs/RUN.md](docs/RUN.md) | เปิดระบบทีละขั้น + ใช้งานหน้าเว็บ |
| [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) | Clean Architecture + flow การตัดสินใจ |
| [docs/OLLAMA.md](docs/OLLAMA.md) | เลือก LLM ให้เหมาะกับ MacBook Pro M-series 24GB |

## เริ่มเร็วสุด

```bash
# 1) บริการพื้นฐาน
docker compose up -d postgres
ollama pull qwen3:14b        # ดู docs/OLLAMA.md

# 2) สามส่วน (คนละ terminal)
python3 ai-layer/http_server.py         # AI sidecar  :8765
cd backend && cargo run                 # backend     :8080  (migrate อัตโนมัติ)
cd frontend && npm install && npm run dev   # web      :5173

# เปิด http://localhost:5173  → modal ใส่ Bitkub key จะเด้งให้เอง
```

> อยากเห็นแกน AI ทำงานก่อน (ไม่ต้องลง Rust/DB): `python3 ai-layer/cli.py BTC ETH`

## โครงสร้างโปรเจกต์

```
quorum/
├── ai-layer/          # Python — agents + aggregator + judge (รันเดี่ยวได้)
│   ├── agents/        #   technical · trend_ml · finbert · news
│   ├── providers/     #   bitkub (ราคา) · news_provider (ข่าว)
│   ├── aggregator.py  #   consensus voting + veto
│   ├── judge.py       #   Ollama + fallback
│   ├── pipeline.py    #   วงจร 1 รอบ
│   ├── http_server.py #   HTTP ให้ Rust เรียก  (cli.py สำหรับ terminal)
│   └── tests/
├── backend/           # Rust — Clean Architecture
│   └── src/
│       ├── domain/         # models + ports (trait) — แก่นล้วน
│       ├── application/     # TradingService · Watcher · risk
│       ├── infrastructure/  # ai_sidecar · bitkub · postgres · events
│       └── presentation/    # axum handlers · ws · routes
├── frontend/          # SolidJS + TailwindCSS (เว็บ)
│   └── src/components/ # CredentialsModal · WatchList · ConsensusView ...
├── db/migrations/     # PostgreSQL schema
├── config/quorum.toml # mode · symbols · consensus · judge · risk
├── docker-compose.yml # postgres (+ ollama profile)
├── docs/              # BUILD · RUN · ARCHITECTURE · OLLAMA
└── Makefile
```

## ความปลอดภัยที่บังคับ

- default `signal-only` (ไม่ส่งคำสั่งจริง) · Bitkub live order ปิดไว้จนกว่าจะผ่าน backtest
- API key ใส่ผ่าน modal → เก็บใน PostgreSQL ผ่าน backend, **ไม่เคยส่ง secret กลับออก API**
- IQ Option = simulation เท่านั้น (ไม่มี official API, เสี่ยง ToS/ban)
- risk layer override ได้ · ทุกการตัดสินใจมี audit ครบใน DB

## เครื่องมือ (tools) ระดับโปร

- 🔎 **ค้นหา autocomplete + ราคาสด** — `GET /api/symbols/search?q=` และ `GET /api/symbols/ticker/{symbol}`
  (ดึง Bitkub ticker จริง: last, %24h, high/low, volume — cache 15s)
- 🧠 **Reasoning trace** — เห็น "สิ่งที่ AI คิด" ทุกขั้น (data → web → แต่ละ agent → consensus → judge)
  พร้อม **thinking ของ qwen3 จริง** ที่ดึงออกมาแสดง
- 🌐 **Web search ให้ Judge** — ดึงข้อมูลสดจาก DuckDuckGo (keyless) ป้อนเข้า judge
  เพื่อไม่ให้พึ่งความรู้เก่าในโมเดล (โชว์วันที่ปัจจุบัน + snippet ล่าสุด)

## ฟีเจอร์ครบวงจร — ดู [docs/FEATURES.md](docs/FEATURES.md)

เมนู: Live · ประวัติวิเคราะห์ · ประวัติเทรด · กระเป๋าจำลอง · สแกนตลาด · บัญชีจริง · Report · ตั้งค่า

## Roadmap

1. ✅ AI layer + consensus + judge (council 5 ตัว + qwen3 + thinking + web search)
2. ✅ Web app (SolidJS/Tailwind) + Rust backend (clean arch) + PostgreSQL + live WS
3. ✅ Autocomplete + ราคาสด + reasoning trace timeline
4. ✅ Bitkub private API จริง (HMAC) — ดูยอดบัญชี + ส่งคำสั่ง
5. ✅ ซื้อขายอัตโนมัติ + โหมด paper/live + กระเป๋าจำลอง (reset ได้)
6. ✅ ตั้งค่าเทรดละเอียด + ประวัติเทรดผูกกับการวิเคราะห์
7. ✅ AI สแกนหาตลาดเอง (market discovery)
8. ⏳ backtesting harness (พิสูจน์บนข้อมูลย้อนหลัง)
9. ⏳ reliability ของ agent ตาม track record · take-profit/stop-loss auto-exit
10. ⏳ แจ้งเตือน (LINE/Telegram) เมื่อเทรด/เจอโอกาส
```
