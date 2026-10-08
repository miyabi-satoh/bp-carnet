// 「オープンβテスト中」の表示 (docs/architecture.md)。`[server] beta_notice` のときに出す。
// e2e の backend は出さない開発・テストの構成なので、出す構成は `/auth/providers` の応答を差し替えて確かめる。
import { mockAuthProviders } from './auth-mocks';
import { test, expect, NO_LOGIN } from './fixtures';
import { HomePage } from './pages/home-page';
import { LoginPage } from './pages/login-page';
import { SignupPage } from './pages/signup-page';

test.describe('未ログインの画面', () => {
	test.use({ storageState: NO_LOGIN });

	test('ログイン画面は、バッジだけを出す', async ({ page }) => {
		await mockAuthProviders(page, { betaNotice: true });
		const login = new LoginPage(page);
		await login.open();

		await expect(login.betaBadge).toBeVisible();
		await expect(login.betaNotice).toHaveCount(0);
	});

	test('サインアップ画面は、一文を出す', async ({ page }) => {
		await mockAuthProviders(page, { betaNotice: true });
		const signup = new SignupPage(page);
		await signup.open();

		await expect(signup.betaNotice).toBeVisible();
	});

	test('出さない構成では、どの画面にも出さない', async ({ page }) => {
		await mockAuthProviders(page, {});
		const login = new LoginPage(page);
		await login.open();
		await expect(login.submitButton).toBeVisible();
		await expect(login.betaBadge).toHaveCount(0);

		const signup = new SignupPage(page);
		await signup.open();
		await expect(signup.submitButton).toBeVisible();
		await expect(signup.betaNotice).toHaveCount(0);
	});
});

test('ログイン後のヘッダーは、ロゴの横にバッジを出す', async ({ page }) => {
	await mockAuthProviders(page, { betaNotice: true });
	const home = new HomePage(page);
	await home.open();

	await expect(home.header.betaBadge).toBeVisible();
});

test('出さない構成や、構成を問い合わせられないときは、ヘッダーにバッジを出さない', async ({
	page
}) => {
	await mockAuthProviders(page, {});
	const home = new HomePage(page);
	await home.open();
	await expect(home.header.userMenuButton).toBeVisible();
	await expect(home.header.betaBadge).toHaveCount(0);

	await page.unroute('**/api/v1/auth/providers');
	await page.route('**/api/v1/auth/providers', (route) => route.fulfill({ status: 503 }));
	await page.reload();
	await expect(home.header.userMenuButton).toBeVisible();
	await expect(home.header.betaBadge).toHaveCount(0);
});
