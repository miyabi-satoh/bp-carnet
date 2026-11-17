-- ログインの成功と失敗の記録 (docs/authentication.md「ログインの記録と知らせ」)。不正なログインに本人と運営者が
-- 気づき、後から、いつ・どこから入られたかを調べられるようにする。90日を過ぎた行は定期タスクが消す。
--
-- user_id: 実在しないユーザーIDへの失敗は NULL (打たれた値を残さないため)。アカウントを消すと行も消える。
-- method: password | google | line | apple | signup (サインアップの確定)。
-- client: web | app。
-- ip: 接続元が分からない構成では NULL。
-- device_hash: ウェブの成功だけ。端末の印の Cookie の値の SHA-256。新しい端末からのログインかの判定に使う。
CREATE TABLE auth_events (
    id INTEGER PRIMARY KEY,
    user_id INTEGER REFERENCES users (id) ON DELETE CASCADE,
    kind TEXT NOT NULL CHECK (kind IN ('login', 'login_failed')),
    method TEXT NOT NULL CHECK (method IN ('password', 'google', 'line', 'apple', 'signup')),
    client TEXT NOT NULL CHECK (client IN ('web', 'app')),
    ip TEXT,
    device_hash TEXT,
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);

CREATE INDEX idx_auth_events_user_id ON auth_events (user_id, created_at);
CREATE INDEX idx_auth_events_created_at ON auth_events (created_at);
