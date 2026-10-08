-- 朝/夜の統計・グラフ (docs/design.md 4章) に必要な、ユーザーごとの設定を追加する。
--
-- 閾値は「その日の 0:00 からの経過分数」で持つ。時刻文字列より比較・加減算が単純なため。
-- 24:00 を終了時刻に指定できるよう上限は 1440。
--
-- ALTER TABLE ADD COLUMN の DEFAULT が既存行にも入るため、別途の backfill は不要。
--
-- 値域・開始<終了・朝夜の非重複は CHECK 制約ではなく src/validation.rs で検証する。
-- 複数列にまたがる条件を SQLite の ALTER TABLE ADD COLUMN では書けないことに加え、
-- 血圧値の値域と同じくバリデーションを1箇所に集約するため。
ALTER TABLE users ADD COLUMN timezone TEXT NOT NULL DEFAULT 'Asia/Tokyo';
ALTER TABLE users ADD COLUMN morning_start_min INTEGER NOT NULL DEFAULT 240;
ALTER TABLE users ADD COLUMN morning_end_min INTEGER NOT NULL DEFAULT 600;
ALTER TABLE users ADD COLUMN evening_start_min INTEGER NOT NULL DEFAULT 1080;
ALTER TABLE users ADD COLUMN evening_end_min INTEGER NOT NULL DEFAULT 1440;
