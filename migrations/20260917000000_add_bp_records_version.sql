-- bp_records.version を追加する (docs/design.md 3章・10章)。
--
-- 記録を直すたびに増やす版番号。画面を開いた時点の値を直す・消す・取り込むときに送り、
-- ほかの端末などで記録が変わっていたら止める。今ある記録は 1 から始める。
ALTER TABLE bp_records ADD COLUMN version INTEGER NOT NULL DEFAULT 1;
