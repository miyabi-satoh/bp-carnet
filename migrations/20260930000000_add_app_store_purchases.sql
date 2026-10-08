-- アプリ内課金 (iPhone・iPad) で買った OCR 枠を記録する (docs/design.md 7章「アプリ内課金 (iPhone・iPad)」)。
--
-- users.apple_app_account_token: 購入オプションの appAccountToken (UUID、小文字)。初めて買うときに作る。
-- ADD COLUMN では UNIQUE を付けられないので、一意の索引で代える。
ALTER TABLE users ADD COLUMN apple_app_account_token TEXT;
CREATE UNIQUE INDEX idx_users_apple_app_account_token ON users (apple_app_account_token);

-- stripe_checkout_session_id の NOT NULL を外すため、ocr_quota_grants を作り直す (SQLite は列の制約を変えられない)。
-- 今の行はすべて Stripe の枠なので、Apple の列は空のまま写す。
--
-- apple_environment: Production | Sandbox。審査・TestFlight の購入 (Sandbox) を売上と見分ける。
-- revoked_milli_yen: 返金で消した残り。空でないことが取り消した行の印で、Apple が返金を取り消したら残りに戻す。
-- apple_signed_date: 行に反映した取引の signedDate (App Store が署名した UNIX 時刻、ミリ秒)。これより古い取り直しでは行を変えない。
CREATE TABLE ocr_quota_grants_new (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    user_id INTEGER NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    granted_milli_yen INTEGER NOT NULL CHECK (granted_milli_yen > 0),
    remaining_milli_yen INTEGER NOT NULL CHECK (remaining_milli_yen >= 0),
    purchased_at TEXT NOT NULL,
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

INSERT INTO ocr_quota_grants_new (id, user_id, granted_milli_yen, remaining_milli_yen, purchased_at, stripe_checkout_session_id)
SELECT id, user_id, granted_milli_yen, remaining_milli_yen, purchased_at, stripe_checkout_session_id
FROM ocr_quota_grants;

-- AUTOINCREMENT の連番を引き継ぐ。写しただけでは写した中の最大の id になり、末尾の行が消えていると
-- (アカウント削除の CASCADE) その id が別の購入にもう一度振られるため。
INSERT INTO sqlite_sequence (name, seq)
SELECT 'ocr_quota_grants_new', 0
WHERE NOT EXISTS (SELECT 1 FROM sqlite_sequence WHERE name = 'ocr_quota_grants_new');
UPDATE sqlite_sequence
SET seq = MAX(seq, COALESCE((SELECT seq FROM sqlite_sequence WHERE name = 'ocr_quota_grants'), 0))
WHERE name = 'ocr_quota_grants_new';

DROP TABLE ocr_quota_grants;
ALTER TABLE ocr_quota_grants_new RENAME TO ocr_quota_grants;
CREATE INDEX idx_ocr_quota_grants_available ON ocr_quota_grants (user_id, id);
