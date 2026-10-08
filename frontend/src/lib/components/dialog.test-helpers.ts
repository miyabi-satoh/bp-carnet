// ダイアログの単体テスト (*.svelte.spec.ts) で共有する探し方。

/** 右上の ×。結果画面の「閉じる」と文言が同じなので、shadcn の slot で探す。 */
export function closeIcon(): HTMLButtonElement {
	const button = document.querySelector<HTMLButtonElement>('[data-slot=dialog-close]');
	if (!button) throw new Error('× が見つからない');
	return button;
}
