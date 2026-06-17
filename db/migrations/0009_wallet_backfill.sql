-- เดิม account_wallet ถูก seed เฉพาะบัญชี paper → บัญชี live ไม่มีแถว
-- ทำให้ GET /api/wallet (view() ใช้ fetch_one) ตอบ 500 Internal Server Error
-- backfill ให้ทุกบัญชีที่ยังไม่มีกระเป๋า (ใช้ค่า default ของตาราง: 100000)
INSERT INTO account_wallet (account_id)
SELECT a.id FROM accounts a
LEFT JOIN account_wallet w ON w.account_id = a.id
WHERE w.account_id IS NULL
ON CONFLICT DO NOTHING;
