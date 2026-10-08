-- users.deletion_scheduled_at を追加する (docs/design.md 10章)。
--
-- NULL 以外なら削除予定 (猶予期間中)。猶予期間中のログインでクリアし、期間を過ぎた行は
-- 定期タスクが物理削除する (docs/design.md 2.6)。
--
-- 他の日時列と同じ UTC の RFC3339 文字列で持ち、期限の判定も文字列の大小で行うため、
-- 書き込む値は秒精度に揃える。
ALTER TABLE users ADD COLUMN deletion_scheduled_at TEXT;
