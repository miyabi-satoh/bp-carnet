import type { Locator, Page } from '@playwright/test';
import { ja } from '../helpers';

/** 写真の読み取りを買い足す前の確認ダイアログ (topup-confirm-dialog.svelte)。 */
export class TopupConfirmDialog {
	readonly root: Locator;

	constructor(page: Page) {
		this.root = page.getByRole('dialog', { name: ja.topup_confirm_title });
	}

	/** 「同意して購入へ進む」。押すと Stripe へ移る。 */
	get agreeButton(): Locator {
		return this.root.getByRole('button', { name: ja.topup_confirm_agree_button, exact: true });
	}
}
