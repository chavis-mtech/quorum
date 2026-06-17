# เลือก Ollama Model สำหรับ Judge (MacBook Pro M-series, RAM 24GB)

Judge มีหน้าที่ **อ่านเสียงโหวตที่ขัดแย้งกันแล้วตอบเป็น JSON สั้น ๆ** (action/confidence/
reasoning) ไม่ใช่งานเขียนยาว → ต้องการโมเดลที่ **ทำตาม instruction + ออก JSON ได้นิ่ง**,
**ใช้เหตุผลชั่งน้ำหนักได้ดี**, และ **เร็วพอ** เพราะรันหลายตัวต่อรอบ

> บริบทเครื่อง: Apple Silicon, unified memory 24GB — แชร์กับ OS + browser + (อาจมี)
> FinBERT/torch ที่กินอีก ~2GB ดังนั้น **อย่าจองหมด** เหลือ headroom ไว้
>
> หมายเหตุ: เอกสารนี้อิงข้อมูลถึง Qwen3 (ออกเม.ย. 2025) — ถ้ามีรุ่นใหม่กว่าใน Ollama
> library ตอนคุณอ่าน ใช้ได้เลย หลักการเลือก (พิกัด RAM/ความเร็ว) ยังเหมือนเดิม

## คำแนะนำ (Qwen3)

| อันดับ | Model | ขนาด (Q4) | เหมาะกับ | คำสั่ง |
|--------|-------|-----------|----------|--------|
| ⭐ หลัก | **qwen3:14b** | ~9 GB | คุณภาพ JSON/เหตุผลดีที่สุดในพิกัดที่ "ปลอดภัย" ยังเหลือ RAM | `ollama pull qwen3:14b` |
| 🔥 คุณภาพสุด | **qwen3:30b-a3b** | ~18 GB | MoE (30B total / 3B active) → เก่งระดับโมเดลใหญ่ แต่เร็วเหมือนตัวเล็ก | `ollama pull qwen3:30b-a3b` |
| 🚀 สำรอง/เร็ว | **qwen3:8b** | ~5 GB | เครื่องโหลดหนัก/อยากตอบไว ไว้ fallback | `ollama pull qwen3:8b` |

**สรุป:** ตั้ง `qwen3:14b` เป็นหลัก, `qwen3:8b` เป็น fallback ที่เร็ว

ตั้งใน [`config/quorum.toml`](../config/quorum.toml):

```toml
[judge]
provider = "ollama"
model    = "qwen3:14b"
```

## ทำไมหลักเป็น 14b ไม่ใช่ 30b-a3b (ทั้งที่ MoE ดูดีกว่า)?

Ollama เก็บโมเดลค้างใน memory (`keep_alive`) — ต้องคิดงบ RAM ของ **ทุกอย่างที่ค้างพร้อมกัน**:

| สูตร | รวม | สถานะใน 24GB |
|------|-----|--------------|
| qwen3:14b (9) + FinBERT (2) + OS/browser (8) | ~19 GB | ✅ สบาย |
| qwen3:30b-a3b (18) + FinBERT (2) + OS/browser (8) | ~28 GB | ❌ เกิน → swap → ช้า |

→ จะใช้ `qwen3:30b-a3b` ได้คุ้ม **ต่อเมื่อ** ย้าย FinBERT ไปใช้ API หรือไม่รัน sentiment
model แบบ local พร้อมกัน (เหลือ RAM ให้ MoE ~18GB)

## Thinking mode (จุดเด่นของ Qwen3)

Qwen3 เปิด/ปิด "การคิด" ได้:

- **เปิด thinking** → judge ชั่งน้ำหนักเสียงขัดแย้งได้ลึกขึ้น แต่ช้าลง (เสีย token คิดก่อนตอบ)
- **ปิด** (ใส่ `/no_think` ใน prompt หรือ option) → เร็วกว่ามาก เหมาะตอน watcher วนถี่หลาย symbol

แนวทาง: วิเคราะห์ on-demand ทีละตัว → เปิด thinking; watch loop อัตโนมัติถี่ ๆ → ปิด

## ทำไมไม่ qwen2.5 / ไม่ตัวใหญ่กว่า 30B?

- **qwen2.5** ยังใช้ได้ แต่ Qwen3 เหนือกว่าทั้ง reasoning + structured output → ไม่มีเหตุผลให้เลือกของเก่า
- **qwen3:32b dense** Q4 ≈ 20GB → ตึงกว่า 30b-a3b และช้ากว่า (active params เยอะกว่า) ไม่คุ้ม
- อยากได้ของใหญ่จริง ๆ: ใช้ **API ฟรี (Groq/Gemini)** เป็น primary แล้ว fallback มา local
  จะคุ้มกว่า — Quorum รองรับ fallback chain อยู่แล้ว

## ทิป latency

- โค้ด judge ใช้ `temperature` ต่ำ (0.2) + `format: json` → output นิ่ง คาดเดาได้
- ครั้งแรกที่เรียกจะช้า (โหลดเข้า memory) — เรียก warm-up 1 ครั้งหลังสตาร์ท
- watcher ถี่ + หลาย symbol → พิจารณา `qwen3:8b` เพื่อ throughput

## ตรวจว่าพร้อม

```bash
ollama list                                   # เห็นโมเดลที่ pull ไว้
ollama run qwen3:14b "ตอบ JSON เท่านั้น: {\"ok\":true}"
curl http://localhost:11434/api/tags          # Quorum เช็คผ่าน endpoint นี้
```
