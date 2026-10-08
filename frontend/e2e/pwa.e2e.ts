// PWA (docs/architecture.md)。manifest と Service Worker。ログインは要らないので未ログインで流す。
import { test, expect, NO_LOGIN } from './fixtures';
import { isApiResponse, ja } from './helpers';
import { waitForAuthProviders } from './page-waits';

// playwright.config.ts は Service Worker を止めているため、このファイルでだけ許す。
test.use({ storageState: NO_LOGIN, serviceWorkers: 'allow' });

test('manifest に、アプリの名前・起動の URL・表示のしかた・アイコンがある', async ({ page }) => {
	await page.goto('/login');
	const href = await page.locator('link[rel="manifest"]').getAttribute('href');
	expect(href).toBeTruthy();
	const res = await page.request.get(href!);
	expect(res.ok()).toBe(true);
	const manifest = await res.json();
	expect(manifest).toMatchObject({
		name: ja.app_name,
		short_name: ja.app_name,
		lang: 'ja',
		start_url: '/',
		scope: '/',
		display: 'standalone'
	});

	type Icon = { src: string; sizes: string; purpose?: string };
	const icons: Icon[] = manifest.icons;
	// ホーム画面・起動画面に要る 192px・512px と、端末ごとの形に切り抜かれる maskable。
	expect(icons.map(({ sizes, purpose }) => `${sizes} ${purpose ?? 'any'}`).sort()).toEqual([
		'192x192 any',
		'512x512 any',
		'512x512 maskable'
	]);
	for (const { src } of icons) {
		const icon = await page.request.get(src);
		expect(icon.ok(), src).toBe(true);
		expect(icon.headers()['content-type'], src).toBe('image/png');
	}
});

test('Service Worker が有効になり、画面は Service Worker から、API の応答はサーバーから返る', async ({
	page,
	ignoreHTTPSErrors
}) => {
	// 登録はセキュアコンテキストのときだけ。localhost (`just e2e-local`) はセキュアコンテキストに当たる。
	// 証明書エラーのあるページ (自己署名の証明書の環境) では、`ignoreHTTPSErrors` でもブラウザが登録を拒む。
	test.skip(ignoreHTTPSErrors, '証明書の検証を省く環境では Service Worker が登録されない');

	await page.goto('/login');
	// 有効になると開いている画面を読み直させることがあり、その間の evaluate は失敗するのでやり直す。
	const registration = () =>
		page
			.evaluate(async () => {
				const reg = await navigator.serviceWorker.getRegistration();
				return {
					state: reg?.active?.state ?? null,
					script: reg?.active ? new URL(reg.active.scriptURL).pathname : null,
					controlled: navigator.serviceWorker.controller !== null
				};
			})
			.catch(() => null);
	await expect
		.poll(registration)
		.toEqual({ state: 'activated', script: '/sw.js', controlled: true });

	// 静的アセットだけをキャッシュし、API の応答はキャッシュしない (共有端末でログアウト後にデータを残さないため)。
	const apiFromServiceWorker: boolean[] = [];
	page.on('response', (res) => {
		if (new URL(res.url()).pathname.startsWith('/api/')) {
			apiFromServiceWorker.push(res.fromServiceWorker());
		}
	});
	const providers = waitForAuthProviders(page);
	const navigation = await page.reload();
	expect(navigation?.fromServiceWorker()).toBe(true);
	expect(isApiResponse(await providers, 'GET', '/auth/providers')).toBe(true);
	expect(apiFromServiceWorker.length).toBeGreaterThan(0);
	expect(apiFromServiceWorker).not.toContain(true);
});
