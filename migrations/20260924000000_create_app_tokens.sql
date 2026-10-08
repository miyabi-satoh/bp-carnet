-- モバイルアプリのログイン (docs/design.md 2.8)。トークン本体は持たずハッシュだけを持つ。
--
-- session_generation: 発行したときの users.session_generation。ずれたら無効 (パスワード変更で他の端末を断つ、→2.4)。
-- last_used_at: 最後に使った時刻 (UTC)。ここから30日で失効する。
CREATE TABLE app_tokens (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    user_id INTEGER NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    token_hash TEXT NOT NULL UNIQUE,
    session_generation INTEGER NOT NULL,
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    last_used_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);
CREATE INDEX idx_app_tokens_user_id ON app_tokens (user_id);
