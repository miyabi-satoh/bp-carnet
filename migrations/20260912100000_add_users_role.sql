-- users.role を追加する (docs/design.md 10章)。
--
-- CHECK 制約を付けるのは、'user' | 'admin' 以外が入ると Rust 側のデコード (auth::Role) が
-- 失敗して 500 になるため。値の正しさは書き込み時点で担保する。
--
-- ALTER TABLE ADD COLUMN の DEFAULT が既存行にも入るため、別途の backfill は不要。
ALTER TABLE users ADD COLUMN role TEXT NOT NULL DEFAULT 'user' CHECK (role IN ('user', 'admin'));
