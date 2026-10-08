import type { Locator, Page } from '@playwright/test';
import { ja, requiredLabel } from '../helpers';

/** 本人によるアカウントの削除 (delete-account-dialog.svelte)。 */
export class DeleteAccountDialog {
	readonly root: Locator;

	constructor(private readonly page: Page) {
		this.root = page.getByRole('dialog', { name: ja.delete_account_dialog_title });
	}

	get confirmation(): Locator {
		return this.root.getByLabel(requiredLabel(ja.delete_account_dialog_confirm_label), {
			exact: true
		});
	}

	get submitButton(): Locator {
		return this.root.getByRole('button', {
			name: ja.delete_account_dialog_submit_button,
			exact: true
		});
	}

	/** 予約できたときの結果。同じダイアログの中身が差し替わり、名前も変わる。 */
	get scheduledResult(): Locator {
		return this.page.getByRole('dialog', { name: ja.delete_account_dialog_scheduled_title });
	}
}
