import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

vi.mock('$app/state', () => ({ page: { data: {} } }));
vi.mock('$lib/api/client', () => ({ client: { GET: vi.fn() } }));
import { client } from '$lib/api/client';
import { fetchAuthProviders, fetchContactUrl } from './auth';
import { respond } from '$lib/api/client.test-helpers';

const getMock = client.GET as unknown as ReturnType<typeof vi.fn>;

const providers = {
	googleEnabled: true,
	contactUrl: 'https://example.com/contact'
};

beforeEach(() => {
	vi.useFakeTimers();
	getMock.mockReset();
});

afterEach(() => {
	vi.useRealTimers();
});

describe('fetchAuthProviders', () => {
	it('returns the providers', async () => {
		getMock.mockImplementation(() => respond(200, providers));

		await expect(fetchAuthProviders()).resolves.toEqual({ ok: true, data: providers });
	});

	// ログイン方式が分からないまま「どれも使えない」扱いで出すと、ボタンが黙って消えるため、失敗として返す。
	it('reports offline when the request throws', async () => {
		getMock.mockImplementation(() => Promise.reject(new TypeError('offline')));

		await expect(fetchAuthProviders()).resolves.toEqual({ ok: false, failure: 'offline' });
	});

	it('reports a server failure when the request returns an error', async () => {
		getMock.mockImplementation(() => respond(503, { error: { code: 'internal', message: 'x' } }));

		await expect(fetchAuthProviders()).resolves.toEqual({ ok: false, failure: 'server' });
	});

	// 止まっている本番サーバーが起きるまで (約3秒) は待ち、それを過ぎたら届かなかった扱いにする。
	it('waits past a cold start and reports offline only after the deadline', async () => {
		getMock.mockImplementation(() => new Promise(() => {}));

		let settled = false;
		const providers = fetchAuthProviders().finally(() => (settled = true));
		await vi.advanceTimersByTimeAsync(5000);
		expect(settled).toBe(false);
		await vi.advanceTimersByTimeAsync(5000);

		await expect(providers).resolves.toEqual({ ok: false, failure: 'offline' });
	});
});

describe('fetchContactUrl', () => {
	it('returns the contact URL from the providers', async () => {
		getMock.mockImplementation(() => respond(200, providers));
		await expect(fetchContactUrl()).resolves.toBe('https://example.com/contact');
	});

	it('is null when the request fails or throws', async () => {
		getMock.mockImplementation(() => Promise.reject(new TypeError('offline')));
		await expect(fetchContactUrl()).resolves.toBeNull();
	});

	it('is null when the request misses the deadline', async () => {
		getMock.mockImplementation(() => new Promise(() => {}));

		const url = fetchContactUrl();
		await vi.advanceTimersByTimeAsync(3000);

		await expect(url).resolves.toBeNull();
	});
});
