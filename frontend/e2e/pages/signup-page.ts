import type { Locator, Page } from '@playwright/test';
import { consentText, ja, requiredLabel } from '../helpers';
import { waitForAuthProviders } from '../page-waits';

/** アカウントの作成 (`/signup`)。 */
export class SignupPage {
	constructor(private readonly page: Page) {}

	/** 開き、サインアップを出すかの問い合わせ (`/auth/providers`) の応答まで待つ。 */
	async open(): Promise<void> {
		const providers = waitForAuthProviders(this.page);
		await this.page.goto('/signup');
		await providers;
	}

	get email(): Locator {
		return this.page.getByLabel(requiredLabel(ja.common_email_label), { exact: true });
	}

	get submitButton(): Locator {
		return this.page.getByRole('button', { name: ja.signup_submit_button, exact: true });
	}

	/** 送れたら、フォームに替えて出す「確認メールを送りました」。 */
	get sentTitle(): Locator {
		return this.page.getByText(ja.signup_sent_title, { exact: true });
	}

	/** フォームの上の「オープンβテスト中です…」の一文 (オープンβテスト中の構成だけ)。 */
	get betaNotice(): Locator {
		return this.page.getByText(ja.beta_notice_message, { exact: true });
	}

	/** 送信ボタンの直上の、利用規約への同意の文。 */
	get termsConsent(): Locator {
		return this.page.getByText(consentText(ja.terms_consent_signup), { exact: true });
	}
}
