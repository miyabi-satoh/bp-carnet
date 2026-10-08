-- users.deletion_origin を追加し、deletion_scheduled_at をミリ秒精度に揃える (docs/design.md 2.6・10章)。
--
-- 削除予約の起点。'user' は本人、'admin' は管理者。予約が無ければ NULL。本人のログインで
-- 取り消せるのは本人が起点の予約だけで、管理者が起点の予約は管理者だけが取り消せる。
-- CHECK 制約は users.role と同じく、Rust 側のデコード失敗 (500) を書き込み時点で防ぐため。
--
-- 既存の予約は、この列ができる前の挙動 (本人のログインで取り消せる) に合わせて 'user' とする。
-- 他の日時列 (created_at 等) と同じミリ秒の形に書き直す。期限の判定は文字列の大小で行うため、
-- 秒の形 ('...:00Z') とミリ秒の形 ('...:00.000Z') が混ざると正しく比べられない。
ALTER TABLE users ADD COLUMN deletion_origin TEXT CHECK (deletion_origin IN ('user', 'admin'));

UPDATE users
SET deletion_origin = 'user',
    deletion_scheduled_at = strftime('%Y-%m-%dT%H:%M:%fZ', deletion_scheduled_at)
WHERE deletion_scheduled_at IS NOT NULL;
