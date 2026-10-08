-- users.display_name・avatar_url を追加する (docs/design.md 10章)。Google ログインの都度、
-- profile scope で取得した名前・画像の URL で上書きする (2.1章)。独自ID/PWだけのアカウントは NULL のまま。
ALTER TABLE users ADD COLUMN display_name TEXT;
ALTER TABLE users ADD COLUMN avatar_url TEXT;
