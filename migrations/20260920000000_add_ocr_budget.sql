-- OCR の累計金額の上限 (無料枠。docs/design.md 7章)。
--
-- ocr_budget_yen: ユーザーごとの上限 (円)。NULL は設定 ([ocr] free_budget_yen) の既定値、
-- 負値は無制限、0 は使えない。
-- ocr_spent_milli_yen: これまでの読み取りに使った額 (1/1000円)。API の usage のトークン数から
-- 単価 ([ocr.pricing]) で計算して足す。導入時は全員 0 から数える (これまでの利用額は記録していない)。
ALTER TABLE users ADD COLUMN ocr_budget_yen INTEGER;
ALTER TABLE users ADD COLUMN ocr_spent_milli_yen INTEGER NOT NULL DEFAULT 0;
