-- OCR の日次利用回数の上限 (docs/design.md 7・10章)。Gemini の呼び出し1回につき1カウントする。
--
-- usage_date はユーザーのタイムゾーン (users.timezone) でのローカル日付 (YYYY-MM-DD)。
-- 過去分の行は上限判定に使わないが、利用状況を追えるよう消さずに残す。
CREATE TABLE ocr_daily_usage (
    user_id INTEGER NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    usage_date TEXT NOT NULL,
    count INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (user_id, usage_date)
);

-- ユーザーごとの上限。NULL は設定 ([ocr] daily_limit) の既定値、負値は無制限を表す。
ALTER TABLE users ADD COLUMN ocr_daily_limit INTEGER;
