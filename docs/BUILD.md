# การ Build Quorum

ระบบมี 3 ส่วนที่ build แยกกัน: **Python AI layer**, **Rust backend**, **SolidJS frontend**

## 0. สิ่งที่ต้องมี (prerequisites)

| เครื่องมือ | เวอร์ชัน | ติดตั้ง |
|-----------|---------|---------|
| Python | 3.11+ | มากับ macOS / `brew install python` |
| Rust | 1.80+ | `curl https://sh.rustup.rs -sSf \| sh` |
| Node.js | 20+ | `brew install node` |
| Docker | ล่าสุด | Docker Desktop (สำหรับ PostgreSQL) |
| Ollama | ล่าสุด | `brew install ollama` หรือ https://ollama.com |

> บน macOS Apple Silicon (M-series): ติดตั้ง **Ollama แบบ native** เพื่อใช้ Metal GPU
> (เร็วกว่ารันใน Docker มาก) — ดู [OLLAMA.md](OLLAMA.md)

---

## 1. Python AI layer

รันได้ทันทีด้วย stdlib (ไม่ต้อง build) — แต่แนะนำสร้าง virtualenv:

```bash
cd ai-layer
python3 -m venv .venv && source .venv/bin/activate

# (optional) เปิดโมเดลสำเร็จรูปเต็มประสิทธิภาพ
pip install transformers torch pandas pandas-ta
```

ตรวจว่าใช้ได้:

```bash
python3 ai-layer/tests/test_aggregator.py     # ต้องผ่าน 5/5
python3 ai-layer/cli.py BTC                    # วิเคราะห์ทดสอบ
```

---

## 2. Rust backend

```bash
cd backend
cargo build              # debug
cargo build --release    # production (binary ที่ target/release/quorum)
cargo test               # รัน unit test (risk rules ฯลฯ)
```

> ครั้งแรกจะดาวน์โหลด/คอมไพล์ dependency (axum, sqlx, reqwest...) ใช้เวลาสักครู่
>
> sqlx ใช้ **runtime queries** จึง build ได้โดย **ไม่ต้องต่อ DB ตอน compile**
> (migration จะรันอัตโนมัติตอน start จากโฟลเดอร์ `db/migrations`)

---

## 3. SolidJS frontend

```bash
cd frontend
npm install
npm run build            # ได้ผลลัพธ์ที่ frontend/dist (backend จะ serve โฟลเดอร์นี้)
```

dev mode (hot reload + proxy ไป backend):

```bash
npm run dev              # เปิด http://localhost:5173
```

---

## สรุป build ทั้งหมดในคำสั่งเดียว

```bash
make frontend-build      # build UI
cd backend && cargo build --release
# Python ไม่ต้อง build
```

ต่อไป: ดู [RUN.md](RUN.md) เพื่อเริ่มระบบ
