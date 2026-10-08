import type { Locator, Page } from '@playwright/test';
import { ja } from '../helpers';

/** 操作が済んだあとにダイアログの中身を差し替えて出す結果 (dialog-result.svelte)。見出しがダイアログの名前になる。 */
export class DialogResult {
	readonly root: Locator;

	constructor(page: Page, title: string) {
		this.root = page.getByRole('dialog', { name: title });
	}

	/** 下の「閉じる」。結果には見出しの × を出さない。 */
	get closeButton(): Locator {
		return this.root.getByRole('button', { name: ja.common_close_button, exact: true });
	}
}
