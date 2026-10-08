// 使っている途中でログインが切れたら、ログイン画面へ送る。
import { afterAll, beforeEach, describe, expect, it, vi } from 'vitest';

vi.mock('$app/navigation', () => ({ goto: vi.fn() }));
vi.mock('$app/paths', () => ({ resolve: (path: string) => path }));
// Node には相対 URL の基点が無いので、呼び先のオリジンを与える。
vi.mock('$lib/native-app', () => ({
	isNativeApp: false,
	API_ORIGIN: 'http://localhost',
	appToken: () => Promise.resolve(null)
}));

const fetchMock = vi.hoisted(() => {
	const mock = vi.fn<typeof fetch>();
	vi.stubGlobal('fetch', mock);
	return mock;
});

import { goto } from '$app/navigation';
import { apiFetch, client } from './client';

function error(status: number, code: string) {
	return Response.json({ error: { code, message: code } }, { status });
}

beforeEach(() => {
	fetchMock.mockReset();
	vi.mocked(goto).mockReset();
	vi.mocked(goto).mockResolvedValue(undefined);
});

afterAll(() => {
	vi.unstubAllGlobals();
});

describe('ログインの切れ', () => {
	it('操作の応答で切れていたら、ログイン画面へ送る', async () => {
		fetchMock.mockResolvedValue(error(401, 'unauthorized'));
		await client.GET('/api/v1/settings');
		expect(goto).toHaveBeenCalledWith('/login', { replaceState: true });
	});

	it('同時にいくつ切れても、移動は1回だけ', async () => {
		let arrive: () => void = () => {};
		vi.mocked(goto).mockImplementation(() => new Promise((resolve) => (arrive = resolve)));
		fetchMock.mockImplementation(async () => error(401, 'unauthorized'));
		const pending = Promise.all([
			client.GET('/api/v1/settings'),
			client.GET('/api/v1/records/latest-date')
		]);
		await vi.waitFor(() => expect(goto).toHaveBeenCalled());
		arrive();
		await pending;
		expect(goto).toHaveBeenCalledTimes(1);
	});

	it('本文を読まない呼び出し (apiFetch) でも送る', async () => {
		fetchMock.mockResolvedValue(error(401, 'unauthorized'));
		await apiFetch('/api/v1/records/export');
		expect(goto).toHaveBeenCalledWith('/login', { replaceState: true });
	});

	it('パスワードの誤りなど、ほかの 401 では送らない', async () => {
		fetchMock.mockResolvedValue(error(401, 'invalid_credentials'));
		await client.POST('/api/v1/auth/login', { body: { username: 'a', password: 'b' } });
		expect(goto).not.toHaveBeenCalled();
	});

	it('ログインの確認 (/auth/me) は layout に任せる', async () => {
		fetchMock.mockResolvedValue(error(401, 'unauthorized'));
		await client.GET('/api/v1/auth/me');
		expect(goto).not.toHaveBeenCalled();
	});

	it('呼び出し側は、これまでどおり応答の本文を読める', async () => {
		fetchMock.mockResolvedValue(error(401, 'unauthorized'));
		const { error: body } = await client.GET('/api/v1/settings');
		expect(body).toEqual({ error: { code: 'unauthorized', message: 'unauthorized' } });
	});
});
