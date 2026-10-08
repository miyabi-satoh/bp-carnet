import type { Attachment } from 'svelte/attachments';

/** 全角の英数字・記号 (U+FF01〜U+FF5E) と全角スペースを半角にする。それ以外の文字は変えない
 * (NFKC だと「㈱」なども崩れるため、日本語入力で紛れ込みやすい範囲だけを直す)。 */
export function toHalfWidth(value: string): string {
	return value.replace(/[\uFF01-\uFF5E\u3000]/g, (char) =>
		char === '\u3000' ? ' ' : String.fromCharCode(char.charCodeAt(0) - 0xfee0)
	);
}

/** 入力欄の全角英数字を、打ち終わった時点で半角に直す。
 *
 * 変換中 (IME の未確定) は触らない。書き換えると確定前の文字が崩れるため。書き換えたら
 * `input` を送り直し、`bind:value` にも半角の値を渡す。 */
export const halfWidthInput: Attachment<HTMLInputElement> = (input) => {
	const normalize = (event: Event) => {
		if (event instanceof InputEvent && event.isComposing) return;
		const normalized = toHalfWidth(input.value);
		if (normalized === input.value) return;
		// 1文字ずつの置き換えで長さは変わらないので、カーソルの位置をそのまま戻せる。
		// type="email" は選択範囲を持たない (selectionStart が null)。
		const { selectionStart, selectionEnd } = input;
		input.value = normalized;
		if (selectionStart !== null && selectionEnd !== null) {
			input.setSelectionRange(selectionStart, selectionEnd);
		}
		input.dispatchEvent(new Event('input', { bubbles: true }));
	};
	input.addEventListener('input', normalize);
	// 確定の `input` が `compositionend` より先に届くブラウザがあるため、確定時にも見る。
	input.addEventListener('compositionend', normalize);
	return () => {
		input.removeEventListener('input', normalize);
		input.removeEventListener('compositionend', normalize);
	};
};
