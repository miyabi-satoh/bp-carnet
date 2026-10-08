-- アプリの LINE ログインで受け取った、LINE との連携を取り消すためのアクセストークン (docs/design.md 2.1.1・2.8)。
--
-- iOS の LINE SDK はリフレッシュトークンをアプリに渡さないため、ログインのたびにアクセストークン
-- (発行から30日) を保存し、退会・連携の解除のときにそのまま取り消しに使う。
-- ウェブのログインで保存するリフレッシュトークン (refresh_token) とは別に持ち、取り消しでは両方を試す。
-- 値は refresh_token と同じ鍵で暗号化したもの (`src/token_cipher.rs`)。Google では使わず NULL。
ALTER TABLE oauth_identities ADD COLUMN access_token TEXT;
