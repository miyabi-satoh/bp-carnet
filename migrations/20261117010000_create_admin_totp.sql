-- 管理画面に入るときの確認コード (TOTP、docs/authentication.md「管理者の確認コード」)。
--
-- admin_totp: 管理者ごとに1行。登録はサーバーの端末のコマンドで行う。
--   secret: 認証アプリと共有する秘密を、環境変数 ADMIN_TOTP_KEY から導いた鍵で暗号化したもの。
--   last_step: 最後に通したコードの時間の区切り (Unix 時刻 / 30秒)。同じコードを二度通さないため。
-- app_tokens.admin_verified_at: アプリのトークンで確認コードを通した時刻 (Unix 秒)。ウェブはセッションに持つ。
CREATE TABLE admin_totp (
    user_id INTEGER PRIMARY KEY REFERENCES users (id) ON DELETE CASCADE,
    secret TEXT NOT NULL,
    last_step INTEGER,
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);

ALTER TABLE app_tokens ADD COLUMN admin_verified_at INTEGER;
