import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

const native = vi.hoisted(() => ({ value: false }));
const fetchAnalyticsToken = vi.hoisted(() => vi.fn<() => Promise<string | null>>());
vi.mock('$lib/native-app', () => ({
	get isNativeApp() {
		return native.value;
	}
}));
vi.mock('$lib/auth', () => ({ fetchAnalyticsToken, isPublicRoute: () => true }));

function beacons(): HTMLScriptElement[] {
	return [
		...document.querySelectorAll<HTMLScriptElement>(
			'script[src="https://static.cloudflareinsights.com/beacon.min.js"]'
		)
	];
}

async function start(stillCounted = () => true): Promise<void> {
	const { startWebAnalytics } = await import('./web-analytics');
	await startWebAnalytics(stillCounted);
}

describe('アクセス解析', () => {
	beforeEach(() => {
		vi.resetModules();
		native.value = false;
		fetchAnalyticsToken.mockReset();
	});
	afterEach(() => {
		for (const script of beacons()) script.remove();
	});

	it('トークンがあれば、画面の移動を数えない設定でスクリプトを1回だけ読む', async () => {
		fetchAnalyticsToken.mockResolvedValue('token-1');
		const { startWebAnalytics } = await import('./web-analytics');
		await startWebAnalytics(() => true);
		await startWebAnalytics(() => true);

		expect(beacons()).toHaveLength(1);
		expect(JSON.parse(beacons()[0].dataset.cfBeacon ?? '')).toEqual({
			token: 'token-1',
			spa: false
		});
		expect(fetchAnalyticsToken).toHaveBeenCalledTimes(1);
	});

	it('トークンが無い構成では読まない', async () => {
		fetchAnalyticsToken.mockResolvedValue(null);
		await start();
		expect(beacons()).toHaveLength(0);
	});

	it('トークンが返るまでに数えない画面へ移っていたら読まない', async () => {
		fetchAnalyticsToken.mockResolvedValue('token-1');
		await start(() => false);
		expect(beacons()).toHaveLength(0);
	});

	it('アプリでは問い合わせもしない', async () => {
		native.value = true;
		fetchAnalyticsToken.mockResolvedValue('token-1');
		await start();
		expect(fetchAnalyticsToken).not.toHaveBeenCalled();
		expect(beacons()).toHaveLength(0);
	});
});
