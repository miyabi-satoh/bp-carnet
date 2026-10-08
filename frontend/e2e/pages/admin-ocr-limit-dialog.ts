import type { Locator, Page } from '@playwright/test';
import { ja, requiredLabel } from '../helpers';

/** 管理者による OCR の上限の変更 (admin-ocr-limit-dialog.svelte)。 */
export class AdminOcrLimitDialog {
	readonly root: Locator;

	constructor(page: Page) {
		this.root = page.getByRole('dialog', { name: ja.admin_users_ocr_limit_dialog_title });
	}

	/** 累計の金額の上限の決め方 (既定・無制限・金額を指定)。 */
	get budgetMode(): Locator {
		return this.root.getByLabel(ja.admin_users_ocr_budget_mode_label, { exact: true });
	}

	/** 累計の金額 (円)。`budgetMode` で金額を指定したときだけ出る。 */
	get budget(): Locator {
		return this.root.getByLabel(requiredLabel(ja.admin_users_ocr_budget_label), { exact: true });
	}

	get submitButton(): Locator {
		return this.root.getByRole('button', {
			name: ja.admin_users_ocr_limit_submit_button,
			exact: true
		});
	}
}
