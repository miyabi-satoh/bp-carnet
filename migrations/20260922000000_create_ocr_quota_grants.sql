-- OCR の購入枠を購入単位で管理する。expires_at は確定した失効規則に従って設定する。
CREATE TABLE ocr_quota_grants (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    user_id INTEGER NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    granted_milli_yen INTEGER NOT NULL CHECK (granted_milli_yen > 0),
    remaining_milli_yen INTEGER NOT NULL CHECK (remaining_milli_yen >= 0),
    purchased_at TEXT NOT NULL,
    expires_at TEXT NOT NULL,
    stripe_checkout_session_id TEXT NOT NULL UNIQUE
);

CREATE INDEX idx_ocr_quota_grants_available
    ON ocr_quota_grants (user_id, expires_at, id);

CREATE TABLE stripe_webhook_events (
    event_id TEXT PRIMARY KEY,
    status TEXT NOT NULL CHECK (status IN ('received', 'processed', 'failed')),
    event_type TEXT NOT NULL,
    checkout_session_id TEXT,
    received_at TEXT NOT NULL,
    processed_at TEXT,
    failure_reason TEXT
);
