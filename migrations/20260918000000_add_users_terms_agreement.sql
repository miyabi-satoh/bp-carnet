-- users に、利用規約・プライバシーポリシーに同意した記録を追加する (docs/design.md 2.2)。
--
-- クラウド版 (`[terms] enabled`) のサインアップ・Google での登録で、同意した規約の版と日時 (UTC、ISO 8601) を残す。
-- 同意を取らずに作ったアカウント (セルフホスト版・管理者や端末から作ったもの) は NULL のまま。
ALTER TABLE users ADD COLUMN terms_version TEXT;
ALTER TABLE users ADD COLUMN terms_agreed_at TEXT;
