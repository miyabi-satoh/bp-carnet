-- `just spec` (アプリ仕様書生成) 用の開発データシード。
-- 一時DB(BP_CARNET_HOMEを一時ディレクトリに向けて起動したbp-carnet)に対して
-- generate-spec.ts が node:sqlite で直接流し込む。Rust側の変更は不要。
--
-- password_hash は "zxcvbnm1" の argon2id ハッシュを固定値としてハードコードしている。
-- ハッシュ文字列自体にsalt/パラメータが埋め込まれているため、hash_password()の
-- 実装が変わらない限り再生成不要。作り直す場合は対話コマンドで一度ユーザーを作り、
-- 開発用DBから password_hash 列を採取する:
--   sqlite3 "$HOME/Library/Application Support/com.amiiby.bp-carnet/bp-carnet.db" \
--     "SELECT password_hash FROM users WHERE username='admin'"

-- email_verified は既定が 0 (未確認のサインアップ) で、未確認のユーザーは管理画面の一覧に数えられない。CLI で作ったユーザーと同じく 1 にする。
INSERT INTO users (username, password_hash, role, email_verified) VALUES
	('admin', '$argon2id$v=19$m=19456,t=2,p=1$DVh8jbYfZupLwm4WFaKGYw$BSbp7sIUyZd4k5ClS8Rf8Z8hn+J20Fl5mwHt06+uaf8', 'admin', 1);

-- ユーザー管理画面の一覧に、状態の違う行を並べるためのユーザー。
-- 血圧記録は持たせない (他の画面のショットは admin でログインするため影響しない)。
INSERT INTO users (username, password_hash, display_name, frozen, email_verified) VALUES
	('hahaue@example.com', '$argon2id$v=19$m=19456,t=2,p=1$DVh8jbYfZupLwm4WFaKGYw$BSbp7sIUyZd4k5ClS8Rf8Z8hn+J20Fl5mwHt06+uaf8', '母', 0, 1),
	('chichiue@example.com', '$argon2id$v=19$m=19456,t=2,p=1$DVh8jbYfZupLwm4WFaKGYw$BSbp7sIUyZd4k5ClS8Rf8Z8hn+J20Fl5mwHt06+uaf8', '父', 1, 1);

-- measured_at は UTC の RFC3339 文字列(アプリ側の正規化と同じ形式)。固定日時にすると
-- 実行日から離れるほどグラフのx軸表示が不自然になり、いずれ「今週/今月」ナビゲーションの
-- 対象からも外れてしまうため、相対指定でシードする。
--
-- トップページの初期表示は「週タブ・最新の記録を含む週」で、明日以降の記録は
-- 最新の日に数えない。そこで `just spec` をいつ実行しても1つの週(日曜始まり)に収まり、すべて
-- 過去になる日付が要る (今週を基準にすると、日曜には記録がすべて明日以降になる)。かつ admin の朝/夜の
-- 閾値は既定のJST 04:00-10:00 / 18:00-24:00 (migrations/20260909100000_...) なので、
-- 時刻もその範囲に収まらないと bpSummary.days が空になりグラフ・統計サマリーが
-- 出ない。そこで「先週の日曜 0:00 (JST)」を基準(week_anchor)にした固定オフセット+
-- 固定時刻(JST)で組み立て、実行時刻には依存しないようにする。
-- (`now`のUTC文字列に対する`+9 hours`はJSTへの近似固定オフセットで、UTC+9のJSTには
-- 夏時間が無いため常に厳密に正しい)。
WITH jst_now(ts) AS (
	SELECT datetime('now', '+9 hours')
),
week_anchor(d) AS (
	SELECT date((SELECT ts FROM jst_now), '-' || (strftime('%w', (SELECT ts FROM jst_now)) + 7) || ' days')
)
INSERT INTO bp_records (user_id, measured_at, systolic, diastolic, pulse, memo)
SELECT (SELECT id FROM users WHERE username = 'admin'),
	strftime('%Y-%m-%dT%H:%M:%fZ', datetime(date((SELECT d FROM week_anchor), '+1 days') || ' 07:00:00', '-9 hours')),
	132, 85, 68, ''
UNION ALL
SELECT (SELECT id FROM users WHERE username = 'admin'),
	strftime('%Y-%m-%dT%H:%M:%fZ', datetime(date((SELECT d FROM week_anchor), '+2 days') || ' 07:30:00', '-9 hours')),
	128, 82, 70, '朝食前'
UNION ALL
SELECT (SELECT id FROM users WHERE username = 'admin'),
	strftime('%Y-%m-%dT%H:%M:%fZ', datetime(date((SELECT d FROM week_anchor), '+3 days') || ' 20:00:00', '-9 hours')),
	135, 88, 72, ''
UNION ALL
SELECT (SELECT id FROM users WHERE username = 'admin'),
	strftime('%Y-%m-%dT%H:%M:%fZ', datetime(date((SELECT d FROM week_anchor), '+5 days') || ' 20:30:00', '-9 hours')),
	124, 79, 65, '散歩後'
UNION ALL
SELECT (SELECT id FROM users WHERE username = 'admin'),
	strftime('%Y-%m-%dT%H:%M:%fZ', datetime(date((SELECT d FROM week_anchor), '+6 days') || ' 08:00:00', '-9 hours')),
	130, 84, 69, '';
