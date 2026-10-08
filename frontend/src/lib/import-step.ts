import { pushState, replaceState } from '$app/navigation';
import { page } from '$app/state';

// 「編集 → 取り込み内容の確認」の2段階のページ (CSV の取り込みのページ・写真で記録のページ) で
// 共通の、確認の段階を履歴に積む扱い。

/** 確認の段階へ進んだ履歴か。 */
export function atPreviewStep(): boolean {
	return page.state.importStep === 'preview';
}

/** 確認の段階へ進む。 */
export function pushPreviewStep(): void {
	pushState('', { ...page.state, importStep: 'preview' });
}

/** 確認の段階の履歴を消す。読み込む対象を選び直したとき、編集の段階から始めるために使う。
 *
 * 消えるのは `page.state` ごとで、確認の段階の印だけではない (記録フォームのシート・写真の拡大表示の印も消える)。
 * 対象を選び直す時点ではどれも積まれていないため。 */
export function clearPreviewStep(): void {
	if (page.state.importStep) replaceState('', {});
}

/** 見出しの戻るボタン。確認の段階なら編集の段階へ戻り、そうでなければ `leave` でページを離れる。
 *
 * 確認の段階で再読み込みすると、履歴 (`page.state`) だけが残って画面は最初の段階に戻るため、
 * 表示が確認の段階か (`previewing`) と履歴の両方を見る。 */
export function backFromPreview(previewing: boolean, leave: () => void): void {
	if (previewing && atPreviewStep()) {
		history.back();
	} else {
		leave();
	}
}

/** 取り込み・登録を終えて元の画面へ戻るときに渡す結果。登録した記録の日付の範囲と件数。 */
export type ImportResult = { from: string; to: string; created: number };

/** 元の画面へ渡す結果。モジュールの変数で持つ (`$lib/file-handoff` と同じ)。 */
let pendingResult: ImportResult | null = null;

/** 元の画面へ戻る前に、取り込み・登録の結果を控える。 */
export function setImportResult(result: ImportResult): void {
	pendingResult = result;
}

/** `setImportResult` で控えた結果を取り出して消す。無ければ `null`。 */
export function takeImportResult(): ImportResult | null {
	const result = pendingResult;
	pendingResult = null;
	return result;
}

/** 取り込みのページを離れる。元の画面から開いて履歴を積んできたとき (`openedFromOrigin`) は、確認の段階へ進んだ履歴の分も
 * 戻り、取り込みのページを履歴に残さない。そうでなければ `goOrigin` で元の画面へ移る。 */
export function leaveImportPage(openedFromOrigin: boolean, goOrigin: () => void): void {
	if (openedFromOrigin) {
		history.go(atPreviewStep() ? -2 : -1);
	} else {
		goOrigin();
	}
}
