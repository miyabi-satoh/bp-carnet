import type { Locator, Page } from '@playwright/test';
import { ja, requiredLabel } from '../helpers';

/** パスワードの再設定の申し込み (`/reset-password`)。 */
export class ResetPasswordPage {
	constructor(private readonly page: Page) {}

	async open(): Promise<void> {
		await this.page.goto('/reset-password');
	}

	get submitButton(): Locator {
		return this.page.getByRole('button', { name: ja.reset_password_request_submit_button });
	}

	get email(): Locator {
		return this.page.getByLabel(requiredLabel(ja.common_email_label), { exact: true });
	}

	/** 送れたら、フォームに替えて出す「メールを送りました」。 */
	get sentTitle(): Locator {
		return this.page.getByText(ja.reset_password_sent_title, { exact: true });
	}

	// 以下はメールのリンク (`/reset-password?token=...`) から開いた画面。

	get newPassword(): Locator {
		return this.page.getByLabel(requiredLabel(ja.common_new_password_label), { exact: true });
	}

	get newPasswordConfirmation(): Locator {
		return this.page.getByLabel(requiredLabel(ja.common_new_password_confirm_label), {
			exact: true
		});
	}

	get completeButton(): Locator {
		return this.page.getByRole('button', { name: ja.reset_password_submit_button, exact: true });
	}

	/** 新しいパスワードを2回入れて「再設定する」を押す。 */
	async setNewPassword(password: string): Promise<void> {
		await this.newPassword.fill(password);
		await this.newPasswordConfirmation.fill(password);
		await this.completeButton.click();
	}

	get doneTitle(): Locator {
		return this.page.getByText(ja.reset_password_done_title, { exact: true });
	}

	/** 期限切れ・使用済みのリンクを開いたときの「このリンクは使えません」。 */
	get linkExpiredTitle(): Locator {
		return this.page.getByText(ja.email_link_expired_title, { exact: true });
	}

	/** 期限切れ・使用済みのリンクから、再設定を申し込み直す画面へ進むボタン。 */
	get retryButton(): Locator {
		return this.page.getByRole('link', { name: ja.reset_password_expired_retry_button });
	}
}
