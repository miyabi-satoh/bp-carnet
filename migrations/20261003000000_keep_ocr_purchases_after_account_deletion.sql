-- アカウントを消しても、購入の記録を残す (docs/design.md 7章「OCR 枠の買い足し」、knowledge の accounts.md)。
-- 消した後に届く返金・不審請求の知らせを引けるようにし、適格請求書の写しと帳簿の保存期間 (7年) に合わせる。
--
-- user_id: アカウントを消すと NULL にし、残りは下のトリガーで 0 にする (未使用分は消える)。
-- price_yen: 買い手が払った額 (円、税込み)。これまでの購入は、ウェブもアプリもすべて300円。
-- 7年を過ぎた持ち主の無い行は、定期タスクが消す (`crate::payments::ocr_quota::purge_orphaned`)。
CREATE TABLE ocr_quota_grants_new (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    user_id INTEGER REFERENCES users (id) ON DELETE SET NULL,
    granted_milli_yen INTEGER NOT NULL CHECK (granted_milli_yen > 0),
    remaining_milli_yen INTEGER NOT NULL CHECK (remaining_milli_yen >= 0),
    purchased_at TEXT NOT NULL,
    price_yen INTEGER NOT NULL CHECK (price_yen > 0),
    stripe_checkout_session_id TEXT UNIQUE,
    apple_transaction_id TEXT UNIQUE,
    apple_environment TEXT,
    revoked_milli_yen INTEGER CHECK (revoked_milli_yen >= 0),
    apple_signed_date INTEGER,
    CHECK (remaining_milli_yen + COALESCE(revoked_milli_yen, 0) <= granted_milli_yen),
    CHECK ((stripe_checkout_session_id IS NULL) <> (apple_transaction_id IS NULL)),
    CHECK ((apple_transaction_id IS NULL AND apple_environment IS NULL AND apple_signed_date IS NULL)
        OR (apple_transaction_id IS NOT NULL AND apple_environment IN ('Production', 'Sandbox')
            AND apple_signed_date IS NOT NULL))
);

INSERT INTO ocr_quota_grants_new (id, user_id, granted_milli_yen, remaining_milli_yen, purchased_at, price_yen,
    stripe_checkout_session_id, apple_transaction_id, apple_environment, revoked_milli_yen, apple_signed_date)
SELECT id, user_id, granted_milli_yen, remaining_milli_yen, purchased_at, 300,
    stripe_checkout_session_id, apple_transaction_id, apple_environment, revoked_milli_yen, apple_signed_date
FROM ocr_quota_grants;

-- AUTOINCREMENT の連番を引き継ぐ (20260930000000_add_app_store_purchases.sql と同じ理由)。
INSERT INTO sqlite_sequence (name, seq)
SELECT 'ocr_quota_grants_new', 0
WHERE NOT EXISTS (SELECT 1 FROM sqlite_sequence WHERE name = 'ocr_quota_grants_new');
UPDATE sqlite_sequence
SET seq = MAX(seq, COALESCE((SELECT seq FROM sqlite_sequence WHERE name = 'ocr_quota_grants'), 0))
WHERE name = 'ocr_quota_grants_new';

-- これまでの Stripe の返金・不審請求は残りを 0 にするだけで、取り消した印を付けていなかった。
-- 処理済みの知らせから印を付け直す (残りは 0 なので CHECK を満たす)。
UPDATE ocr_quota_grants_new SET revoked_milli_yen = 0
WHERE revoked_milli_yen IS NULL
  AND stripe_checkout_session_id IN (
      SELECT checkout_session_id FROM stripe_webhook_events
      WHERE status = 'processed' AND event_type IN ('charge.refunded', 'charge.dispute.created'));

DROP TABLE ocr_quota_grants;
ALTER TABLE ocr_quota_grants_new RENAME TO ocr_quota_grants;
CREATE INDEX idx_ocr_quota_grants_available ON ocr_quota_grants (user_id, id);
CREATE INDEX idx_ocr_quota_grants_orphaned ON ocr_quota_grants (purchased_at) WHERE user_id IS NULL;

-- 持ち主が消えた行の残りを消す。どの経路でアカウントを消しても (猶予の後の削除・未確認のアカウントの掃除)
-- 同じになるよう、外部キーの SET NULL に掛ける。
CREATE TRIGGER ocr_quota_grants_clear_orphaned_remaining
AFTER UPDATE OF user_id ON ocr_quota_grants
WHEN NEW.user_id IS NULL AND NEW.remaining_milli_yen > 0
BEGIN
    UPDATE ocr_quota_grants SET remaining_milli_yen = 0 WHERE id = NEW.id;
END;
