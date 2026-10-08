// 利用規約・プライバシーポリシー (docs/authentication.md)。
// 同意の文は LINE・Google・Apple のボタンにも掛かるので、使う構成を `/auth/providers` の応答を差し替えて確かめる。
// 同意が記録されることは backend の結合テスト (tests/api.rs) で確かめる。
import type { BrowserContext } from '@playwright/test';
import { mockAuthProviders, PRODUCTION_AUTH_PROVIDERS } from './auth-mocks';
import { test, expect, NO_LOGIN } from './fixtures';
import { consentText, isAbove, ja } from './helpers';
import { LegalPage } from './pages/legal-page';
import { LoginPage } from './pages/login-page';
import { SignupPage } from './pages/signup-page';

test.use({ storageState: NO_LOGIN });

/** 本番の構成 (LINE・Google・Apple のログイン) と答える。別のタブにも効くよう、コンテキストに掛ける。 */
async function mockProductionProviders(context: BrowserContext): Promise<void> {
	await mockAuthProviders(context, { ...PRODUCTION_AUTH_PROVIDERS, appleEnabled: true });
}

test.describe('本番の構成', () => {
	test.beforeEach(async ({ context }) => {
		await mockProductionProviders(context);
	});

	test('ログイン画面は LINE・Google・Apple のボタンの直上に同意の文を1つ出し、規約は別のタブで開く', async ({
		page,
		context
	}) => {
		const login = new LoginPage(page);
		await login.open();

		await expect(login.termsConsent).toHaveText(consentText(ja.terms_consent_providers));
		expect(await isAbove(login.termsConsent, login.lineButton)).toBe(true);
		// LINE のボタンを Google の上に置く。
		expect(await isAbove(login.lineButton, login.googleButton)).toBe(true);

		const opened = context.waitForEvent('page');
		await login.termsConsentLink(ja.terms_title).click();
		const termsTab = await opened;
		await termsTab.waitForURL('/terms');
		const terms = new LegalPage(termsTab);
		await expect(terms.heading).toHaveText(ja.terms_title);
		// 別のタブには戻る履歴が無いので、戻るボタンはログイン画面へ移る。
		await terms.backButton.click();
		await termsTab.waitForURL('/login');
	});

	test('ログイン画面のカードの下のリンクから、規約とポリシーを読める', async ({ page }) => {
		const login = new LoginPage(page);
		await login.open();

		await login.footerLink(ja.terms_title).click();
		await page.waitForURL('/terms');
		const terms = new LegalPage(page);
		await expect(terms.heading).toHaveText(ja.terms_title);
		await expect(terms.sectionHeading('第3条（医療の代わりではありません）')).toBeVisible();
		// 本文の中からプライバシーポリシーへ移れる。
		await terms.link(ja.privacy_title).first().click();
		await page.waitForURL('/privacy');
		await expect(terms.heading).toHaveText(ja.privacy_title);
		// 米国の事業者への委託の表を出す。
		await expect(terms.tables.nth(1)).toContainText('Fly.io, Inc.');
		// 同じタブで来たときは、戻るボタンで前のページへ戻る。
		await terms.backButton.click();
		await page.waitForURL('/terms');
	});

	test('アカウントの作成は、送信ボタンの直上に同意の文を出す', async ({ page }) => {
		const signup = new SignupPage(page);
		await signup.open();

		await expect(signup.termsConsent).toHaveText(consentText(ja.terms_consent_signup));
		expect(await isAbove(signup.termsConsent, signup.submitButton)).toBe(true);
	});

	test('アプリの外から開いた規約のページは、履歴があっても戻るボタンで外へ出ずログイン画面へ移る', async ({
		page
	}) => {
		// 読み込み直しを挟んで開き、アプリの中の移動ではない (外から来た) 状態にする。
		await page.goto('/help');
		await page.goto('/terms');
		const terms = new LegalPage(page);
		await expect(terms.heading).toHaveText(ja.terms_title);

		await terms.backButton.click();
		await page.waitForURL('/login');
	});
});

test('構成を問い合わせられなくても、規約のページは本文を出す', async ({ page }) => {
	await page.route('**/api/v1/auth/providers', (route) => route.fulfill({ status: 503 }));
	await page.goto('/privacy');

	await expect(new LegalPage(page).body).toBeVisible();
});
