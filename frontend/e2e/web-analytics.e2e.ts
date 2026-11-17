// アクセス解析 (docs/architecture.md「アクセス解析」)。e2e の backend は計測しない構成なので、計測する構成は
// `/auth/providers` の応答と画面の CSP を差し替え、Cloudflare のスクリプトの読み込みは外へ出さずに数える。
import type { Page } from '@playwright/test';
import { mockAuthProviders } from './auth-mocks';
import { test, expect, NO_LOGIN } from './fixtures';
import { HomePage } from './pages/home-page';
import { LoginPage } from './pages/login-page';

/**
 * 画面の CSP に、計測する構成の backend が足す出どころ (`src/security_headers.rs` の `page_csp`) を足す。
 * 計測しない構成の CSP のままだと、スクリプトの読み込みをブラウザが止めるため。
 */
async function allowAnalyticsInCsp(page: Page): Promise<void> {
	await page.route(
		(url) => !url.pathname.startsWith('/api/') && !url.pathname.startsWith('/_app/'),
		async (route) => {
			if (route.request().resourceType() !== 'document') return route.fallback();
			const response = await route.fetch();
			const csp = (response.headers()['content-security-policy'] ?? '')
				.replace(/script-src ([^;]*)/, 'script-src $1 https://static.cloudflareinsights.com')
				.replace(/connect-src ([^;]*)/, 'connect-src $1 https://cloudflareinsights.com');
			await route.fulfill({
				response,
				headers: { ...response.headers(), 'content-security-policy': csp }
			});
		}
	);
}

/** Cloudflare のスクリプトの読み込みを空の応答で受け、読んだ回数を返す関数を返す。 */
async function countBeaconLoads(page: Page): Promise<() => number> {
	let loads = 0;
	await page.route('https://static.cloudflareinsights.com/**', (route) => {
		loads += 1;
		return route.fulfill({ contentType: 'text/javascript', body: '' });
	});
	return () => loads;
}

test.describe('ログインしていない画面', () => {
	test.use({ storageState: NO_LOGIN });

	test('計測する構成では、スクリプトを読む', async ({ page }) => {
		await mockAuthProviders(page, { cloudflareAnalyticsToken: 'token-1' });
		await allowAnalyticsInCsp(page);
		const loads = await countBeaconLoads(page);
		await new LoginPage(page).open();

		await expect.poll(loads).toBe(1);
	});
});

test('ログインした後の画面では、計測する構成でもスクリプトを読まない', async ({ page }) => {
	await mockAuthProviders(page, { cloudflareAnalyticsToken: 'token-1' });
	const loads = await countBeaconLoads(page);
	await new HomePage(page).open();

	expect(loads()).toBe(0);
});
