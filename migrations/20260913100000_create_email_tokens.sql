-- メール確認・パスワードリセットのリンクに載せるトークン (docs/design.md 2.2・10章)。
--
-- トークン本体は保存せず、SHA-256 のハッシュだけを持つ。DB が漏れてもリンクを作り直せない
-- ようにするため。
--
-- ADR: 同じユーザー・用途につき1行だけにする (UNIQUE)。発行し直すとこの行を上書きし、前に
-- 送ったリンクは使えなくなる。制約で持たせることで、同時に発行されても有効なトークンが
-- 2つ残らない。
CREATE TABLE email_tokens (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    user_id INTEGER NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    purpose TEXT NOT NULL,
    token_hash TEXT NOT NULL UNIQUE,
    expires_at TEXT NOT NULL,
    used_at TEXT,
    UNIQUE (user_id, purpose)
);
