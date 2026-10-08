import type { Locator, Page } from '@playwright/test';
import { ja } from '../helpers';

/** 連携の解除の確認 (unlink-identity-dialog.svelte)。`providerLabel` は「LINE」「Google」。 */
export class UnlinkIdentityDialog {
	readonly root: Locator;

	constructor(
		private readonly page: Page,
		providerLabel: string
	) {
		this.root = page.getByRole('dialog', {
			name: ja.unlink_identity_dialog_title.replace('{provider}', providerLabel)
		});
	}

	get submitButton(): Locator {
		return this.root.getByRole('button', {
			name: ja.unlink_identity_dialog_submit_button,
			exact: true
		});
	}

	get cancelButton(): Locator {
		return this.root.getByRole('button', { name: ja.common_cancel_button, exact: true });
	}

	/** LINE の解除にだけ出る、LINE 側も取り消す旨の注記。 */
	get lineNote(): Locator {
		return this.root.getByText(ja.unlink_identity_dialog_line_note, { exact: true });
	}

	/** 解除に失敗したときのエラーダイアログ。 */
	get errorDialog(): Locator {
		return this.page.getByRole('alertdialog', { name: ja.unlink_identity_dialog_error_title });
	}
}
