// Google でのログイン (docs/authentication.md)。Google との往復は e2e では通せないので、ボタンの出し分けと、
// コールバックが失敗して戻ったときの案内だけを確かめる。ログインそのものは backend の結合テスト (tests/api.rs) で確かめる。
// e2e の backend は Google を使わない構成なので、使う構成は `/auth/providers` の応答を差し替えて確かめる。
import { mockAuthProviders } from './auth-mocks';
import { test, expect, NO_LOGIN } from './fixtures';
import { ja } from './helpers';
import { LoginPage } from './pages/login-page';

test.use({ storageState: NO_LOGIN });

test('Google を使う構成では、ログインのボタンから backend のログインの開始へ移る', async ({
	page
}) => {
	await mockAuthProviders(page, { googleEnabled: true });
	const login = new LoginPage(page);
	await login.open();

	await expect(login.googleButton).toHaveAttribute('href', '/api/v1/auth/google/login');
	await expect(login.lineButton).toHaveCount(0);
});

test('Google を使わない構成では、ボタンを出さない', async ({ page }) => {
	const login = new LoginPage(page);
	await login.open();

	await expect(login.submitButton).toBeVisible();
	await expect(login.googleButton).toHaveCount(0);
});

for (const [code, message] of [
	['google_failed', ja.error_google_failed],
	['google_disabled', ja.error_google_disabled],
	['google_account_conflict', ja.error_google_account_conflict]
] as const) {
	test(`Google のコールバックが ${code} で戻ったら、ログイン画面で理由を知らせる`, async ({
		page
	}) => {
		await mockAuthProviders(page, { googleEnabled: true });
		const login = new LoginPage(page);
		await login.openWithOAuthError(code);

		await expect(login.errorDialog).toContainText(message);
		// 読み込み直しでまた出ないよう、URL からコードを取り除く。
		await expect(page).toHaveURL('/login');
	});
}
