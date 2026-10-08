/** カードの上に置くセクションの見出し。設定画面と、管理者による利用者の
 * 閲覧ページで使う。 */
export const SECTION_TITLE_CLASS = 'mb-2 text-sm font-bold text-muted-foreground';

/** カードの中の一覧の1行。設定画面と使い方の目次で使う。 */
export const LIST_ROW_CLASS =
	'flex min-h-13 w-full items-center justify-between gap-2 px-4 py-3 text-left text-base transition-colors outline-none hover:bg-muted focus-visible:bg-muted disabled:pointer-events-none disabled:opacity-50';

/** 入力欄 (`Input`) と同じ見た目のネイティブの select。幅は使う側で付ける。管理画面の OCR 上限と、
 * 写真で記録の手帳の年で使う。 */
export const FIELD_SELECT_CLASS =
	'h-11 rounded-sm border border-input bg-field px-2.5 text-base outline-none focus-visible:border-ring focus-visible:ring-3 focus-visible:ring-ring/50 md:text-sm';
