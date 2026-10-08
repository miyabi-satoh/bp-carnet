-- users.timezone_auto を追加する (docs/design.md 4章・10章)。
--
-- 1 (自動) なら、アプリを開いたブラウザのタイムゾーンで users.timezone を更新する。users.timezone には
-- 自動のときも具体的な IANA 名を持ち、サーバーの計算はその値で行う。
-- 今いるユーザーも自動にする (手動の設定を守る必要のある利用者がまだいないため)。
ALTER TABLE users ADD COLUMN timezone_auto INTEGER NOT NULL DEFAULT 1 CHECK (timezone_auto IN (0, 1));
