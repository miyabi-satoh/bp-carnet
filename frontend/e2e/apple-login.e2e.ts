// Sign in with Apple (docs/authentication.md)。Apple との往復は e2e では通せないので、ボタンの出し分けと、
// コールバックが失敗して戻ったときの案内だけを確かめる。ログインそのものは backend の結合テスト (tests/api.rs) で確かめる。
// e2e の backend は Apple を使わない構成なので、使う構成は `/auth/providers` の応答を差し替えて確かめる。
import { mockAuthProviders } from './auth-mocks';
import { test, expect, NO_LOGIN } from './fixtures';
import { isAbove, ja } from './helpers';
import { LoginPage } from './pages/login-page';

test.use({ storageState: NO_LOGIN });

test('Apple を使う構成では、ログインのボタンから backend のログインの開始へ移る', async ({
	page
}) => {
	await mockAuthProviders(page, { appleEnabled: true });
	const login = new LoginPage(page);
	await login.open();

	await expect(login.appleButton).toHaveAttribute('href', '/api/v1/auth/apple/login');
	await expect(login.lineButton).toHaveCount(0);
	await expect(login.googleButton).toHaveCount(0);
});

test('Apple のボタンは Google の下に置く', async ({ page }) => {
	await mockAuthProviders(page, { lineEnabled: true, googleEnabled: true, appleEnabled: true });
	const login = new LoginPage(page);
	await login.open();

	expect(await isAbove(login.googleButton, login.appleButton)).toBe(true);
});

test('Apple を使わない構成では、ボタンを出さない', async ({ page }) => {
	const login = new LoginPage(page);
	await login.open();

	await expect(login.submitButton).toBeVisible();
	await expect(login.appleButton).toHaveCount(0);
});

test('Apple のボタンは、明るい画面では黒地、ダークモードでは白地にする', async ({ page }) => {
	await mockAuthProviders(page, { appleEnabled: true });
	const login = new LoginPage(page);
	await login.open();
	await expect(login.appleButton).toHaveCSS('background-color', 'rgb(0, 0, 0)');

	await page.emulateMedia({ colorScheme: 'dark' });
	await login.open();
	await expect(login.appleButton).toHaveCSS('background-color', 'rgb(255, 255, 255)');
});

test.describe('LINE の内蔵ブラウザで開いたとき (docs/authentication.md)', () => {
	test.use({
		userAgent:
			'Mozilla/5.0 (iPhone; CPU iPhone OS 26_6 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Mobile/15E148 Safari Line/15.0.0'
	});

	test('Apple のボタンも出さず、案内に名前を書く', async ({ page }) => {
		await mockAuthProviders(page, { appleEnabled: true });
		const login = new LoginPage(page);
		await login.open();

		await expect(login.inAppBrowserNotice('Apple')).toBeVisible();
		await expect(login.appleButton).toHaveCount(0);
	});
});

for (const [code, message] of [
	['apple_failed', ja.error_apple_failed],
	['apple_disabled', ja.error_apple_disabled],
	['apple_account_conflict', ja.error_apple_account_conflict]
] as const) {
	test(`Apple のコールバックが ${code} で戻ったら、ログイン画面で理由を知らせる`, async ({
		page
	}) => {
		await mockAuthProviders(page, { appleEnabled: true });
		const login = new LoginPage(page);
		await login.openWithOAuthError(code);

		await expect(login.errorDialog).toContainText(message);
		// 読み込み直しでまた出ないよう、URL からコードを取り除く。
		await expect(page).toHaveURL('/login');
	});
}
