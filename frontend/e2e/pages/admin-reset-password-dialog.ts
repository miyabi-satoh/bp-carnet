import type { Locator, Page } from '@playwright/test';
import { ja, requiredLabel } from '../helpers';
import { DialogResult } from './dialog-result';

/** 管理者によるパスワードの再設定 (admin-reset-password-dialog.svelte)。 */
export class AdminResetPasswordDialog {
	readonly root: Locator;
	/** 再設定できたときの結果。 */
	readonly done: DialogResult;

	constructor(page: Page) {
		this.root = page.getByRole('dialog', { name: ja.admin_users_reset_password_dialog_title });
		this.done = new DialogResult(page, ja.admin_users_reset_password_done_title);
	}

	get newPassword(): Locator {
		return this.root.getByLabel(requiredLabel(ja.common_new_password_label), { exact: true });
	}

	get newPasswordConfirmation(): Locator {
		return this.root.getByLabel(requiredLabel(ja.common_new_password_confirm_label), {
			exact: true
		});
	}

	get submitButton(): Locator {
		return this.root.getByRole('button', {
			name: ja.admin_users_reset_password_submit_button,
			exact: true
		});
	}
}
