import type { Locator, Page } from '@playwright/test';
import { ja, requiredLabel } from '../helpers';
import { DialogResult } from './dialog-result';

/** 管理者によるユーザーの追加 (admin-add-user-dialog.svelte)。 */
export class AdminAddUserDialog {
	readonly root: Locator;
	/** 追加できたときの結果。 */
	readonly done: DialogResult;

	constructor(page: Page) {
		this.root = page.getByRole('dialog', { name: ja.admin_users_add_dialog_title });
		this.done = new DialogResult(page, ja.admin_users_add_done_title);
	}

	get username(): Locator {
		return this.root.getByLabel(requiredLabel(ja.common_username_label), { exact: true });
	}

	get password(): Locator {
		return this.root.getByLabel(requiredLabel(ja.common_password_label), { exact: true });
	}

	get passwordConfirmation(): Locator {
		return this.root.getByLabel(requiredLabel(ja.common_password_confirm_label), { exact: true });
	}

	get submitButton(): Locator {
		return this.root.getByRole('button', { name: ja.admin_users_add_submit_button, exact: true });
	}

	/** 見出しの ×。 */
	get closeButton(): Locator {
		return this.root.getByRole('button', { name: ja.common_close_button, exact: true });
	}
}
