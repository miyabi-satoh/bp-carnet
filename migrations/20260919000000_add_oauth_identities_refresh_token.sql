-- LINE との連携を取り消すためのリフレッシュトークン (docs/design.md 2.1)。
--
-- LINE の開発ガイドラインは、退会時に連携を取り消す (deauthorize) ことを必須にしている。
-- 取り消しには利用者のアクセストークンが要るので、ログインのたびにリフレッシュトークンを保存し、
-- 物理削除のときにアクセストークンを取り直して取り消す。
-- 値はチャネルシークレットから導いた鍵で暗号化したもの (`src/token_cipher.rs`)。Google では使わず NULL。
ALTER TABLE oauth_identities ADD COLUMN refresh_token TEXT;
