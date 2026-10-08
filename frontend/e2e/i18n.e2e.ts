// 表示言語の決め方 (docs/architecture.md)。localStorage → ブラウザの表示言語 → 日本語 の順で、今は日本語しか無い。
// 保存先を見るため、何も残っていない未ログインの状態から始める。
import { test, expect, NO_LOGIN } from './fixtures';
import { LoginPage } from './pages/login-page';

const LOCALE_KEY = 'PARAGLIDE_LOCALE';

test.use({ storageState: NO_LOGIN, locale: 'en-US' });

test('ブラウザの表示言語がアプリに無い言語なら日本語で開き、決まった言語を cookie ではなく localStorage に残す', async ({
	page,
	context
}) => {
	const login = new LoginPage(page);
	await login.open();

	await expect(login.submitButton).toBeVisible();
	await expect(page.locator('html')).toHaveAttribute('lang', 'ja');
	expect(await page.evaluate((key) => localStorage.getItem(key), LOCALE_KEY)).toBe('ja');
	expect((await context.cookies()).map((cookie) => cookie.name)).not.toContain(LOCALE_KEY);
});
