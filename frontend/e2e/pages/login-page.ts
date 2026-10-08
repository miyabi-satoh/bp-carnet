import type { Locator, Page } from '@playwright/test';
import { consentText, ja } from '../helpers';
import { openLoginPage } from '../page-waits';
import { HomePage } from './home-page';
import { ResetPasswordPage } from './reset-password-page';
import { SignupPage } from './signup-page';

/** ログイン画面 (`/login`)。 */
export class LoginPage {
	constructor(private readonly page: Page) {}

	/** 開き、LINE・Google・Apple ログインを出すかの問い合わせ (`/auth/providers`) の応答まで待つ。 */
	async open(): Promise<void> {
		await openLoginPage(this.page);
	}

	/** LINE・Google・Apple のコールバックが失敗して戻ってきたときの URL (`oauthError` にコード) で開く。 */
	async openWithOAuthError(code: string): Promise<void> {
		await openLoginPage(this.page, `/login?oauthError=${code}`);
	}

	/** ログインできなかった理由を知らせるダイアログ。 */
	get errorDialog(): Locator {
		return this.page.getByRole('alertdialog', { name: ja.login_error_title });
	}

	/** パスワード欄の横の「パスワードを表示」ボタンに当たらないよう、欄は完全一致で探す。サインアップが
	 * 有効なサーバーでは「メールアドレス」、無ければ「ユーザーID」と書かれる。 */
	get username(): Locator {
		return this.page
			.getByLabel(ja.common_email_label, { exact: true })
			.or(this.page.getByLabel(ja.common_username_label, { exact: true }));
	}

	get password(): Locator {
		return this.page.getByLabel(ja.common_password_label, { exact: true });
	}

	get submitButton(): Locator {
		return this.page.getByRole('button', { name: ja.login_submit_button, exact: true });
	}

	/** 「LINEでログインする」(backend へのリンク)。 */
	get lineButton(): Locator {
		return this.page.getByRole('link', { name: ja.login_line_button, exact: true });
	}

	/** iOS のホーム画面から開いたアプリで、LINE のボタンの代わりに出す案内。`services` は代わりに勧める方法。 */
	lineIosWebAppNotice(services: string): Locator {
		return this.page.getByText(ja.login_line_ios_web_app_notice.replace('{services}', services), {
			exact: true
		});
	}

	/** 内蔵ブラウザのとき、ボタンの代わりに出す案内。`services` は有効な方法の名前。 */
	inAppBrowserNotice(services: string): Locator {
		return this.page.getByText(ja.login_in_app_browser_notice.replace('{services}', services));
	}

	/** 内蔵ブラウザのとき、LINE・Google・Apple のボタンの代わりに出す「リンクをコピー」ボタン。 */
	get copyLinkButton(): Locator {
		return this.page.getByRole('button', {
			name: ja.login_in_app_browser_copy_button,
			exact: true
		});
	}

	/** 「リンクをコピー」を押したあとに出す知らせ。 */
	get copiedNotice(): Locator {
		return this.page.getByText(ja.login_in_app_browser_copied);
	}

	/** 失敗のダイアログの、問い合わせフォームへのリンク (`contactUrl` がある構成だけ)。 */
	get errorContactLink(): Locator {
		return this.errorDialog.getByRole('link', { name: ja.common_contact_form_link });
	}

	/** 失敗のダイアログの、使い方の項目へのリンク。 */
	get errorHelpLink(): Locator {
		return this.errorDialog.getByRole('link', { name: ja.notice_help_link });
	}

	/** 「Appleでサインイン」(backend へのリンク)。 */
	get appleButton(): Locator {
		return this.page.getByRole('link', { name: ja.login_apple_button, exact: true });
	}

	/** 「Googleでログインする」(backend へのリンク)。 */
	get googleButton(): Locator {
		return this.page.getByRole('link', { name: ja.login_google_button, exact: true });
	}

	/** アプリ名の横の「β」バッジ (オープンβテスト中の構成だけ)。 */
	get betaBadge(): Locator {
		return this.page.getByRole('img', { name: ja.beta_badge_label, exact: true });
	}

	/** 「オープンβテスト中です…」の一文。ログイン画面には出さない (出ていないことを確かめる)。 */
	get betaNotice(): Locator {
		return this.page.getByText(ja.beta_notice_message, { exact: true });
	}

	get helpLink(): Locator {
		return this.footerLink(ja.login_help_link);
	}

	/** 「アカウントを作成」(サインアップを受け付ける構成だけ)。 */
	get signupLink(): Locator {
		return this.page.getByRole('link', { name: ja.login_signup_link, exact: true });
	}

	get forgotPasswordLink(): Locator {
		return this.page.getByRole('link', { name: ja.login_forgot_password_link, exact: true });
	}

	/** LINE・Google・Apple のボタンの直上の、利用規約への同意の文。3つとも出す構成の文言で探す。 */
	get termsConsent(): Locator {
		return this.page.getByText(consentText(ja.terms_consent_providers), { exact: true });
	}

	/** カードの下の、使い方・利用規約・プライバシーポリシーへのリンクの並び (規約は出す構成だけ)。 */
	footerLink(name: string): Locator {
		return this.page
			.getByRole('navigation', { name: ja.login_footer_links_label, exact: true })
			.getByRole('link', { name, exact: true });
	}

	/** 同意の文の中のリンク (`ja.terms_title`・`ja.privacy_title`)。別のタブで開き、読み上げではそう知らせる。 */
	termsConsentLink(name: string): Locator {
		return this.termsConsent.getByRole('link', {
			name: `${name} ${ja.common_opens_in_new_tab}`,
			exact: true
		});
	}

	/** パスワード欄の横の「パスワードを表示」。押すと「パスワードを隠す」に変わる。 */
	get showPasswordButton(): Locator {
		return this.page.getByRole('button', { name: ja.password_input_show_label, exact: true });
	}

	get hidePasswordButton(): Locator {
		return this.page.getByRole('button', { name: ja.password_input_hide_label, exact: true });
	}

	/** 入力済みの欄を送信し、ホーム画面へ移るのを待つ。アカウントの削除の取り消しなど、クエリ付きで移ることがあるのでパスだけを見る。 */
	async submit(): Promise<HomePage> {
		await this.submitButton.click();
		await this.page.waitForURL((url) => url.pathname === '/');
		return new HomePage(this.page);
	}

	/** 「パスワードを忘れた」から再設定の画面へ移るのを待つ。 */
	async openForgotPassword(): Promise<ResetPasswordPage> {
		await this.forgotPasswordLink.click();
		await this.page.waitForURL('/reset-password');
		return new ResetPasswordPage(this.page);
	}

	/** 「アカウントを作成」からサインアップの画面へ移るのを待つ。 */
	async openSignup(): Promise<SignupPage> {
		await this.signupLink.click();
		await this.page.waitForURL('/signup');
		return new SignupPage(this.page);
	}

	/** 開いてログインし、ホーム画面へ移るのを待つ。 */
	async logIn(username: string, password: string): Promise<HomePage> {
		await this.open();
		await this.username.fill(username);
		await this.password.fill(password);
		return this.submit();
	}
}
