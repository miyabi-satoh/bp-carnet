// LINE でのログイン (docs/authentication.md)。LINE との往復は e2e では通せないので、ボタンの出し分けと、
// コールバックが失敗して戻ったときの案内だけを確かめる。ログインそのものは backend の結合テスト (tests/api.rs) で確かめる。
// e2e の backend は LINE を使わない構成なので、使う構成は `/auth/providers` の応答を差し替えて確かめる。
import type { Page } from '@playwright/test';
import { mockAuthProviders } from './auth-mocks';
import { test, expect, NO_LOGIN } from './fixtures';
import { ja } from './helpers';
import { HelpTopicPage } from './pages/help-topic-page';
import { LoginPage } from './pages/login-page';

test.use({ storageState: NO_LOGIN });

/** LINE ログインだけを使う構成と答える。 */
async function mockLineOnlyProviders(page: Page): Promise<void> {
	await mockAuthProviders(page, { lineEnabled: true });
}

test('LINE を使う構成では、ログインのボタンから backend のログインの開始へ移る', async ({
	page
}) => {
	await mockLineOnlyProviders(page);
	const login = new LoginPage(page);
	await login.open();

	await expect(login.lineButton).toHaveAttribute('href', '/api/v1/auth/line/login');
	await expect(login.googleButton).toHaveCount(0);
});

test('LINE を使わない構成では、ボタンを出さない', async ({ page }) => {
	const login = new LoginPage(page);
	await login.open();

	await expect(login.submitButton).toBeVisible();
	await expect(login.lineButton).toHaveCount(0);
});

test('iOS のホーム画面から開いたアプリでは、LINE のボタンの代わりに案内を出す', async ({
	page
}) => {
	// iOS の全画面のウェブアプリだけ `navigator.standalone` が true になる (docs/architecture.md)。
	await page.addInitScript(() => {
		Object.defineProperty(navigator, 'standalone', { value: true, configurable: true });
	});
	await mockAuthProviders(page, { lineEnabled: true, googleEnabled: true, appleEnabled: true });
	const login = new LoginPage(page);
	await login.open();

	await expect(login.lineIosWebAppNotice('Google・Apple')).toBeVisible();
	await expect(login.lineButton).toHaveCount(0);
	await expect(login.googleButton).toBeVisible();
	await expect(login.appleButton).toBeVisible();
	await expect(login.submitButton).toBeVisible();
});

for (const [code, message] of [
	['line_email_required', ja.error_line_email_required],
	['line_failed', ja.error_line_failed],
	['line_disabled', ja.error_line_disabled],
	['line_account_conflict', ja.error_line_account_conflict]
] as const) {
	test(`LINE のコールバックが ${code} で戻ったら、ログイン画面で理由を知らせる`, async ({
		page
	}) => {
		await mockLineOnlyProviders(page);
		const login = new LoginPage(page);
		await login.openWithOAuthError(code);

		await expect(login.errorDialog).toContainText(message);
		// 読み込み直しでまた出ないよう、URL からコードを取り除く。
		await expect(page).toHaveURL('/login');
	});
}

test('コールバックが失敗して戻ったときのダイアログから、原因に合った使い方の項目へ移れる', async ({
	page
}) => {
	await mockLineOnlyProviders(page);
	const login = new LoginPage(page);
	await login.openWithOAuthError('line_email_required');
	await login.errorHelpLink.click();

	await expect(page).toHaveURL('/help/trouble-line-email');
	await expect(new HelpTopicPage(page).heading(ja.help_trouble_line_email_title)).toBeVisible();
});

test('LINE のコールバックが失敗して戻ったときは、内蔵ブラウザの使い方の項目へ移れる', async ({
	page
}) => {
	await mockLineOnlyProviders(page);
	const login = new LoginPage(page);
	await login.openWithOAuthError('line_failed');
	await login.errorHelpLink.click();

	await expect(page).toHaveURL('/help/trouble-in-app-browser');
});

test('使い方では直せない失敗のダイアログには、使い方へのリンクを出さない', async ({ page }) => {
	await mockLineOnlyProviders(page);
	const login = new LoginPage(page);
	await login.openWithOAuthError('line_account_conflict');

	await expect(login.errorDialog).toContainText(ja.error_line_account_conflict);
	await expect(login.errorHelpLink).toHaveCount(0);
});

test('本人では直せない失敗には、運営者への問い合わせとフォームへのリンクを添える', async ({
	page
}) => {
	await mockAuthProviders(page, { lineEnabled: true, contactUrl: 'https://example.com/contact/' });
	const login = new LoginPage(page);
	await login.openWithOAuthError('line_account_conflict');

	await expect(login.errorDialog).toContainText(
		`${ja.error_line_account_conflict}${ja.common_contact_operator}`
	);
	await expect(login.errorContactLink).toHaveAttribute('href', 'https://example.com/contact/');
	await expect(login.errorContactLink).toHaveAttribute('target', '_blank');
});

test.describe('LINE の内蔵ブラウザで開いたとき (docs/authentication.md)', () => {
	// LINE のアプリの中で開くと、User-Agent の末尾に `Line/<版>` が付く。
	test.use({
		userAgent:
			'Mozilla/5.0 (iPhone; CPU iPhone OS 26_6 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Mobile/15E148 Safari Line/15.0.0',
		permissions: ['clipboard-read', 'clipboard-write']
	});

	test('LINE・Google のボタンの代わりに案内を出し、リンクをコピーできる', async ({ page }) => {
		await mockAuthProviders(page, { lineEnabled: true, googleEnabled: true });
		const login = new LoginPage(page);
		await login.open();

		await expect(login.lineButton).toHaveCount(0);
		await expect(login.googleButton).toHaveCount(0);
		await expect(login.submitButton).toBeVisible();

		await login.copyLinkButton.click();
		await expect(login.copiedNotice).toBeVisible();
		expect(await page.evaluate(() => navigator.clipboard.readText())).toBe(
			new URL('/login', page.url()).href
		);
	});
});
