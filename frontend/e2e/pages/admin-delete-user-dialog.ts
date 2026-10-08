import type { Locator, Page } from '@playwright/test';
import { ja, requiredLabel } from '../helpers';
import { DialogResult } from './dialog-result';

/** 管理者によるユーザーの削除の予約 (admin-delete-user-dialog.svelte)。見出しと確認の文言に相手の名前が入る。 */
export class AdminDeleteUserDialog {
	readonly root: Locator;
	/** 予約できたときの結果。 */
	readonly scheduled: DialogResult;

	constructor(
		page: Page,
		private readonly username: string
	) {
		this.root = page.getByRole('dialog', {
			name: ja.admin_users_delete_dialog_title.replace('{name}', username)
		});
		this.scheduled = new DialogResult(page, ja.admin_users_delete_scheduled_title);
	}

	get confirmation(): Locator {
		return this.root.getByLabel(
			requiredLabel(ja.admin_users_delete_confirm_label.replace('{username}', this.username)),
			{ exact: true }
		);
	}

	get submitButton(): Locator {
		return this.root.getByRole('button', {
			name: ja.admin_users_delete_submit_button,
			exact: true
		});
	}
}
