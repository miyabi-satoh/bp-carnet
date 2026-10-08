import type { Locator, Page } from '@playwright/test';
import { ja, requiredLabel } from '../helpers';

/** 確認メールのリンクから開く、パスワードの設定 (`/verify-email?token=...`)。 */
export class VerifyEmailPage {
	constructor(private readonly page: Page) {}

	get password(): Locator {
		return this.page.getByLabel(requiredLabel(ja.common_password_label), { exact: true });
	}

	get passwordConfirmation(): Locator {
		return this.page.getByLabel(requiredLabel(ja.common_password_confirm_label), { exact: true });
	}

	get submitButton(): Locator {
		return this.page.getByRole('button', { name: ja.verify_email_submit_button, exact: true });
	}

	/** パスワードを2回入れて「設定する」を押す。 */
	async setPassword(password: string): Promise<void> {
		await this.password.fill(password);
		await this.passwordConfirmation.fill(password);
		await this.submitButton.click();
	}

	/** 期限切れ・使用済みのリンクを開いたときの「このリンクは使えません」。 */
	get linkExpiredTitle(): Locator {
		return this.page.getByText(ja.email_link_expired_title, { exact: true });
	}

	/** 期限切れ・使用済みのリンクから、確認メールを申し込み直す画面へ進むボタン。 */
	get retryButton(): Locator {
		return this.page.getByRole('link', { name: ja.verify_email_expired_retry_button });
	}
}
