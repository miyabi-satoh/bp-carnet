// インボイスの登録で課税事業者になる日 (docs/payments.md)。
// backend の `src/payments/stripe.rs` の `TAX_INCLUDED_FROM` と同じ日時にする。
// これより前は「消費税なし」、以後は「税込み」と出す。

/** 2026-12-01 0時 (日本時間)。 */
export const TAX_INCLUDED_FROM = new Date('2026-12-01T00:00:00+09:00');

/** `now` の時点で、買い足しの値段が税込みか (課税事業者か)。 */
export function isTaxIncluded(now: Date = new Date()): boolean {
	return now >= TAX_INCLUDED_FROM;
}
