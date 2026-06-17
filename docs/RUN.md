# การ Run Quorum

ต้องเปิด 4 อย่างตามลำดับ: **PostgreSQL → Ollama → Python AI → Rust backend** (+ frontend dev ถ้าพัฒนา UI)

---

## วิธีเร็ว (dev) — เปิดทีละ terminal

### Terminal 1 — PostgreSQL + (Ollama)

```bash
docker compose up -d postgres          # ฐานข้อมูล
# Ollama (Apple Silicon แนะนำ native):
ollama serve                            # ถ้ายังไม่ได้รันเป็น service
ollama pull qwen3:14b        # ครั้งแรกเท่านั้น (ดู OLLAMA.md)
```

### Terminal 2 — Python AI sidecar

```bash
python3 ai-layer/http_server.py
# → [quorum-ai] ฟังที่ http://127.0.0.1:8765
```

### Terminal 3 — Rust backend

```bash
cp .env.example .env                    # ครั้งแรก
cd backend && cargo run
# → migration รันอัตโนมัติ แล้ว "ฟังที่ http://0.0.0.0:8080"
```

### Terminal 4 — Frontend

- **dev (hot reload):** `cd frontend && npm run dev` → เปิด http://localhost:5173
- **production:** `make frontend-build` แล้วเปิด http://localhost:8080 (backend serve UI ให้เอง)

---

## ครั้งแรกที่เปิดเว็บ

1. ระบบเช็คว่ามี Bitkub API key หรือยัง — ถ้ายัง **modal จะเด้งให้ใส่** อัตโนมัติ
   (จะกด "ข้ามไปก่อน" ก็ได้ ระบบยังดูราคา/วิเคราะห์ได้โดยไม่ต้องมี key)
2. ใส่หุ้น/เหรียญที่อยากเฝ้าดูในช่อง WatchList (เช่น `BTC`, `ETH`, `ADA`)
3. กดที่ชื่อสินทรัพย์เพื่อ **วิเคราะห์ทันที** หรือรอ Watcher วนรอบให้เอง
4. แถบบนจะบอกว่า **"กำลังวิเคราะห์ตัวไหนอยู่"** real-time ผ่าน WebSocket
5. แท็บ **Report** ดูสถิติ + ประวัติทั้งหมดที่เก็บใน PostgreSQL

---

## ตรวจสุขภาพระบบ

```bash
curl http://localhost:8080/api/health        # {"ai_engine":true,...}
curl http://localhost:8765/health            # {"ok":true} (Python)
curl -X POST http://localhost:8080/api/analyze -d '{"symbol":"BTC"}'
```

---

## ปรับพฤติกรรม

| อยากเปลี่ยน | แก้ที่ |
|------------|--------|
| โหมด (paper/live/signal-only), หุ้น default, กฎ consensus, risk | [`config/quorum.toml`](../config/quorum.toml) |
| Judge LLM (model/provider/fallback) | `[judge]` ใน `config/quorum.toml` |
| ความถี่เฝ้าราคา / วิเคราะห์ลึก / พักตัวที่ยังไม่มีแผน | `WATCH_INTERVAL_SECS`, `DEEP_INTERVAL_SECS`, `NO_PLAN_COOLDOWN_SECS` ใน `.env` |
| URL ของ DB / sidecar / พอร์ต | `.env` |
| API key ตรวจข่าว (Finnhub/NewsAPI) | env var `FINNHUB_API_KEY` / `NEWSAPI_KEY` |

---

## หมายเหตุความปลอดภัย

- default mode = `signal-only` — **ไม่ส่งคำสั่งจริง** จนกว่าจะตั้งใจเปลี่ยน
- Bitkub `place_order` ปิดไว้ในโค้ดจนกว่าจะผ่าน backtest (roadmap)
- ระบบนี้เป็น **single local workspace**: API key ของแต่ละ broker มี 1 ชุดต่อ DB และ watchlist/ผลวิเคราะห์/แผนเทรดถูกแชร์ให้ทุก client ที่ต่อ backend เดียวกัน
- Bitkub key เก็บใน PostgreSQL ผ่าน backend และ **ไม่เคยถูกส่งกลับออกทาง API**
  (production ควรเพิ่มการเข้ารหัสด้วย pgcrypto/KMS — ดู migration หมายเหตุ)
