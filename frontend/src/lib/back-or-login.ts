import { afterNavigate, goto } from '$app/navigation';
import { resolve } from '$app/paths';

/** 見出しの戻るボタンの動き。アプリの中から移ってきたときだけ前のページへ戻る。最初に開いたページ
 * (同意の文から別のタブで開いた・外のリンクから来た) なら、アプリの外へ出さずログイン画面へ移る。
 *
 * コンポーネントの初期化中に呼ぶ (`afterNavigate` を使うため)。 */
export function backOrLogin(): () => void {
	let cameFromApp = false;
	// FIX: 読み込み直して開いたときも `from` が null にならないことがある (e2e で確かめた) ので、移動の種類で見分ける。
	afterNavigate(({ type }) => {
		cameFromApp = type !== 'enter';
	});

	return () => {
		if (cameFromApp) history.back();
		else goto(resolve('/login'));
	};
}
