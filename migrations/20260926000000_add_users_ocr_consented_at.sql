-- 写真を Gemini API に送ることへの同意 (docs/design.md 7章「写真を送ることへの同意」)。
--
-- ocr_consented_at: 同意した時刻 (UTC)。NULL は未同意 (取り消した場合も NULL に戻す)。
-- 既存ユーザーも NULL から始める (サインアップ時のポリシーへの同意とは別に、送る前に聞き直すため)。
ALTER TABLE users ADD COLUMN ocr_consented_at TEXT;
