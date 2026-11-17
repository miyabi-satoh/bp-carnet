import type { Locator, Page } from '@playwright/test';
import { ja } from '../helpers';

/** 管理画面に入るときの確認コードの入力 (`admin-code-gate.svelte`)。管理画面のどのページにも出る。 */
export class AdminCodeGate {
	constructor(private readonly page: Page) {}

	get code(): Locator {
		return this.page.getByLabel(ja.admin_code_label);
	}

	get submitButton(): Locator {
		return this.page.getByRole('button', { name: ja.admin_code_submit_button, exact: true });
	}

	/** 認証アプリが端末で登録されていないときの案内。 */
	get notEnrolledNotice(): Locator {
		return this.page.getByText(ja.error_admin_code_not_enrolled, { exact: true });
	}

	/** 違うコードを入れたときのエラー。 */
	get invalidCodeError(): Locator {
		return this.page.getByText(ja.error_invalid_admin_code, { exact: true });
	}

	async submit(code: string): Promise<void> {
		await this.code.fill(code);
		await this.submitButton.click();
	}
}
