-- users テーブル (認証層で使用)。
-- セッション用のテーブルは tower-sessions-sqlx-store の SqliteStore::migrate() が
-- 別途作成するため、ここでは定義しない。
--
-- id は AUTOINCREMENT にする: 削除したユーザーの id が新規ユーザーに再利用されると、
-- 削除前に発行された残留セッションが新規ユーザーとして認証されてしまうため。
CREATE TABLE users (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    username TEXT NOT NULL UNIQUE,
    password_hash TEXT NOT NULL,
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);
