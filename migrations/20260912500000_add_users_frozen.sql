-- users.frozen を追加する (docs/design.md 10章)。
--
-- 1 なら凍結中。ユーザー管理画面 (アートボード AdminUsers) が状態バッジに表示する。
-- ログイン・セッション継続で弾く処理は、凍結・解除の API と同時に入れる。
ALTER TABLE users ADD COLUMN frozen INTEGER NOT NULL DEFAULT 0;
