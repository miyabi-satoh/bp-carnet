/** 送信中のモーダルを Escape・外側のクリックで閉じさせないために、`Dialog.Content` に spread する。
 * 閉じられないようにするのは、閉じた後に操作が成立すると結果 (作ったID・削除の予定日など) を
 * 伝えられなくなるため。× は `DialogFormHeader` の `closeDisabled` で別に無効にする。
 *
 * ADR: 送信中かどうかを props の値にせず、渡す関数の中で読む。bits-ui の `Dialog.Content` は props が
 * 変わるとポータルの中身を作り直すため、送信の開始・終了で作り直されて、閉じる途中の中身が
 * 覆いごと残ったり、完了を受け取れなくなったりする。 */
export function busyCloseGuard(isBusy: () => boolean) {
	const preventWhileBusy = (e: Event) => {
		if (isBusy()) e.preventDefault();
	};
	return { onEscapeKeydown: preventWhileBusy, onInteractOutside: preventWhileBusy };
}

/** 戻せない操作の確認ダイアログの実行ボタンに足すクラス。
 * shadcn-svelte の `variant="destructive"` は淡色なので塗りつぶしに上書きする。ダイアログを開くボタン
 * (設定画面の行など) は淡色のまま。 */
export const DESTRUCTIVE_ACTION_CLASS =
	'bg-destructive text-white hover:bg-destructive/90 dark:bg-destructive dark:hover:bg-destructive/90';

/** フォームのダイアログを開いたとき、最初の入力欄にフォーカスを送る。`Dialog.Content` の `onOpenAutoFocus` に渡す。
 * bits-ui の既定は最初のタブ移動先に送るため、見出しの × (`DialogFormHeader`) に当たってしまう。
 * 入力欄がまだ無い (読み込み中) ときは中身そのものに送り、Tab で × から回れるようにする。 */
export function focusFirstFieldOnOpen(getContent: () => HTMLElement | null) {
	return (e: Event) => {
		e.preventDefault();
		requestAnimationFrame(() => {
			const content = getContent();
			const field = content?.querySelector<HTMLElement>(
				'input:not([type=hidden]):not(:disabled), select:not(:disabled), textarea:not(:disabled)'
			);
			(field ?? content)?.focus();
		});
	};
}
