-- 買い足した OCR 枠の失効をやめる (docs/design.md 7章「OCR 枠の買い足し」、2026-09-28 にユーザーが決定)。
-- 使う順は購入の順 (id) にする。
DROP INDEX idx_ocr_quota_grants_available;
ALTER TABLE ocr_quota_grants DROP COLUMN expires_at;
CREATE INDEX idx_ocr_quota_grants_available ON ocr_quota_grants (user_id, id);
