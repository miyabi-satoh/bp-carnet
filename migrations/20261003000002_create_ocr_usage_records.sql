-- 写真の読み取りに使った額を、使うたびに残す (docs/design.md 7章「読み取りに使った額の記録」、knowledge の accounts.md)。
-- 買い足した枠 (前払いの残高) は、買った時でなく使った時に課税売上になり、Stripe・Apple の明細からは使った額を
-- 数えられないため。申告のときに、課税期間の国内の買い手の分を合計する。
--
-- ocr_quota_grants.domestic: 買い手が国内か (1) 国外か (0)。Stripe は Checkout の住所の国、Apple は取引の
-- storefront で決める。分からなかった行と、これまでの行は NULL。
ALTER TABLE ocr_quota_grants ADD COLUMN domestic INTEGER CHECK (domestic IN (0, 1));

-- kind: paid (買った枠。Stripe と Apple の Production) | sandbox (Apple の Sandbox の枠。売上ではない) | free (無料の枠)。
-- grant_id: 減らした購入の行。アカウントを消しても行は残す。購入の行より長く残ることがあるので外部キーにしない。
-- domestic: 減らした購入の行の値を写す (購入の行が消えた後も合計できるように)。paid のときだけ入れる。
-- sales_milli_yen: 使った量を売値に直した税込みの額 (1/1000円)。price_yen × used ÷ granted を切り捨てる。paid のときだけ。
-- アカウントの列は持たない。使った年 (日本時間) の翌年の初めから8年を過ぎた行は、定期タスクが消す。
CREATE TABLE ocr_usage_records (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    used_at TEXT NOT NULL,
    kind TEXT NOT NULL CHECK (kind IN ('paid', 'sandbox', 'free')),
    grant_id INTEGER,
    domestic INTEGER CHECK (domestic IN (0, 1)),
    used_milli_yen INTEGER NOT NULL CHECK (used_milli_yen > 0),
    sales_milli_yen INTEGER NOT NULL CHECK (sales_milli_yen >= 0),
    CHECK ((kind = 'free') = (grant_id IS NULL)),
    CHECK (kind = 'paid' OR (domestic IS NULL AND sales_milli_yen = 0))
);

CREATE INDEX idx_ocr_usage_records_used_at ON ocr_usage_records (used_at);
