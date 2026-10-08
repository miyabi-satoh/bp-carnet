-- OAuth (Google 等) でログインしたユーザーと、外部プロバイダのアカウントを紐付けるテーブル。
--
-- additive 方式: 既存の users テーブル (password_hash NOT NULL) は一切変更しない。
-- OAuth 専用アカウントの users.password_hash には、ランダム32バイトを argon2 でハッシュ化した
-- 「絶対にID/PWログインでマッチしない」値を入れる (Django の set_unusable_password 相当)。
--
-- subject (Google の sub) を一意キーとする。email をキーにしないのは、Google 側でメール
-- アドレスが変更され得る (sub は不変) ため。
--
-- user_id に ON DELETE CASCADE を付けるのは、ユーザー削除時にこのテーブルの行が孤児化する
-- (削除したはずの外部アカウント紐付けが残り続ける) のを防ぐため。bp_records と同じ方針。
-- db.rs で foreign_keys(true) が有効なため実際に効く。
CREATE TABLE oauth_identities (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    user_id INTEGER NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    provider TEXT NOT NULL,
    subject TEXT NOT NULL,
    email TEXT NOT NULL,
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    UNIQUE (provider, subject)
);

-- ユーザー削除時のカスケード先確認・将来の「連携済みプロバイダ一覧」機能などのため、
-- user_id 単体での検索用インデックス (UNIQUE(provider, subject) だけではカバーされない)。
CREATE INDEX idx_oauth_identities_user_id ON oauth_identities (user_id);
