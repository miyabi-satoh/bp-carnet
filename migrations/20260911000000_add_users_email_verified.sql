-- users.email_verified を追加する (docs/design.md 10章)。同一メールアドレスの Google アカウントとの
-- 自動統合 (2.1章) は、確認済みのユーザーだけを対象にする。
--
-- 既定値は 0 (未確認) にし、確認済みとして作る経路 (管理者による作成・Google ログイン) で明示的に
-- 1 を入れる。既定値を 1 にすると、確認を経ずにユーザーを作る経路 (将来のオープンサインアップ等) で
-- 指定を忘れたとき、そのユーザーが統合の対象になってしまうため。
--
-- 既存行は全て管理者による作成か Google ログインで作られたユーザーなので、1 で埋める。
ALTER TABLE users ADD COLUMN email_verified INTEGER NOT NULL DEFAULT 0;
UPDATE users SET email_verified = 1;
