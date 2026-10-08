-- OCR の1日の読み取り回数の上限 (users.ocr_daily_limit・ocr_daily_usage) をなくす (docs/design.md 7章)。
-- 費用は累計金額の上限 (無料枠) と、同じユーザーの読み取りを1件ずつにすることで抑える。
DROP TABLE ocr_daily_usage;
ALTER TABLE users DROP COLUMN ocr_daily_limit;
