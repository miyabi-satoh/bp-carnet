-- 最終アクセスと、自動退会の予告 (docs/design.md 2.6.1)。
--
-- last_seen_at: 最終アクセス (UTC)。NULL は作成時刻 (created_at) として扱う。SQLite は式の既定値を
-- ADD COLUMN に付けられないので NULL 可にし、新しいアカウントは created_at にそのまま任せる。
-- 導入時、既存ユーザーは適用時刻から数え始める (いきなり退会させない)。
-- inactivity_notice_days: 最後に送った予告 (30 / 7 / 1)。最終アクセスの更新で NULL に戻す。
ALTER TABLE users ADD COLUMN last_seen_at TEXT;
ALTER TABLE users ADD COLUMN inactivity_notice_days INTEGER;
UPDATE users SET last_seen_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now');

-- 自動退会の削除予約の起点 'inactive' を許すため、deletion_origin の CHECK 制約を作り直す。
-- SQLite は制約を直接は変えられないので、新しい列へ写して差し替える。
ALTER TABLE users ADD COLUMN deletion_origin_new TEXT CHECK (deletion_origin_new IN ('user', 'admin', 'inactive'));
UPDATE users SET deletion_origin_new = deletion_origin;
ALTER TABLE users DROP COLUMN deletion_origin;
ALTER TABLE users RENAME COLUMN deletion_origin_new TO deletion_origin;
