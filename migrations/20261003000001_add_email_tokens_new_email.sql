-- メールアドレスの変更 (docs/design.md 2.2「メールアドレスの変更」)。確認のリンクを送った新しいアドレスを、
-- トークンと一緒に持つ。purpose = 'change_email' の行だけが値を持つ。
ALTER TABLE email_tokens ADD COLUMN new_email TEXT;
