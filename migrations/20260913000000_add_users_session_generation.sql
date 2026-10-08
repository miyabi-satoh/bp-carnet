-- セッションの世代 (docs/design.md 2.4)。パスワードを変更したときに +1 する。
--
-- ADR: セッションストア (`tower_sessions`) の `data` は MessagePack の BLOB で、SQL から
-- user_id を引けない。そのため「そのユーザーのセッションだけを選んで消す」ことはできず、
-- セッションに書いた世代がこの値と違えば無効として扱う方式にする。
ALTER TABLE users ADD COLUMN session_generation INTEGER NOT NULL DEFAULT 0;
