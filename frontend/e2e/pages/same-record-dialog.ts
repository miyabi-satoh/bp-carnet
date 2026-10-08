import type { Locator, Page } from '@playwright/test';
import { ja } from '../helpers';

/** 同じ日時・同じ値の記録がすでにあるときの確認 (same-record-dialog.svelte)。 */
export class SameRecordDialog {
	readonly root: Locator;

	constructor(page: Page) {
		this.root = page.getByRole('alertdialog', { name: ja.same_record_dialog_title });
	}

	get confirmButton(): Locator {
		return this.root.getByRole('button', {
			name: ja.same_record_dialog_confirm_button,
			exact: true
		});
	}

	get cancelButton(): Locator {
		return this.root.getByRole('button', {
			name: ja.same_record_dialog_cancel_button,
			exact: true
		});
	}
}
