import type { Locator, Page } from '@playwright/test';
import { ja } from '../helpers';

/** ページや入力を離れる前の確認 (leave-confirm-dialog.svelte)。見出しと離れるボタンの文言は使う場所ごとに違う。 */
export class LeaveConfirmDialog {
	readonly root: Locator;

	constructor(
		page: Page,
		private readonly labels: { title: string; leave: string }
	) {
		this.root = page.getByRole('alertdialog', { name: labels.title });
	}

	get stayButton(): Locator {
		return this.root.getByRole('button', {
			name: ja.leave_confirm_dialog_stay_button,
			exact: true
		});
	}

	get leaveButton(): Locator {
		return this.root.getByRole('button', { name: this.labels.leave, exact: true });
	}
}
