-- 血圧記録テーブル。
--
-- measured_at は UTC の RFC3339 文字列で保存する (アプリ層で正規化してから書き込む)。
-- クライアントのタイムゾーンのまま保存すると `ORDER BY measured_at` や将来の期間フィルタ
-- (from/to) が壊れるため、書き込み時に UTC へ正規化する運用とする。
--
-- user_id に ON DELETE CASCADE を付けるのは、ユーザー削除時に記録が孤児化してデータ分離の
-- 前提 (他ユーザーからは絶対に見えない) が崩れるのを防ぐため。db.rs で foreign_keys(true) を
-- 有効化済みなので実際に効く。
CREATE TABLE bp_records (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    user_id INTEGER NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    measured_at TEXT NOT NULL,
    systolic INTEGER NOT NULL,
    diastolic INTEGER NOT NULL,
    pulse INTEGER,
    memo TEXT NOT NULL DEFAULT '',
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);

-- 一覧取得 (ユーザーごとに measured_at 降順) を高速化するための複合インデックス。
CREATE INDEX idx_bp_records_user_id_measured_at ON bp_records (user_id, measured_at);
