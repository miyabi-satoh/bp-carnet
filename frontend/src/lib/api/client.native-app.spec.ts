// アプリ (Capacitor) の中では、公開 URL をトークン付きで呼ぶ (docs/mobile-app.md)。
import { afterAll, beforeEach, describe, expect, it, vi } from 'vitest';

const token = vi.hoisted(() => ({ value: 'tok' as string | null }));
vi.mock('$lib/native-app', () => ({
	isNativeApp: true,
	API_ORIGIN: 'https://bp.example.test',
	appToken: () => Promise.resolve(token.value),
	appVersion: () => Promise.resolve('1.2.3')
}));

// openapi-fetch は作ったときの `fetch` を握るので、読み込む前に差し替える。
const fetchMock = vi.hoisted(() => {
	const mock = vi.fn<typeof fetch>();
	vi.stubGlobal('fetch', mock);
	return mock;
});

import { appUpdate } from '$lib/app-update.svelte';
import { apiFetch, client } from './client';

beforeEach(() => {
	token.value = 'tok';
	appUpdate.required = false;
	fetchMock.mockReset();
	fetchMock.mockResolvedValue(Response.json({}));
});

afterAll(() => {
	vi.unstubAllGlobals();
});

describe('client (アプリ)', () => {
	it('公開 URL をトークン付きで呼ぶ', async () => {
		await client.GET('/api/v1/auth/me');
		const request = fetchMock.mock.calls[0][0] as Request;
		expect(request.url).toBe('https://bp.example.test/api/v1/auth/me');
		expect(request.headers.get('Authorization')).toBe('Bearer tok');
	});

	it('呼び出し側が付けたトークンを上書きしない', async () => {
		await client.POST('/api/v1/app/auth/logout', {
			headers: { Authorization: 'Bearer other' }
		});
		const request = fetchMock.mock.calls[0][0] as Request;
		expect(request.headers.get('Authorization')).toBe('Bearer other');
	});

	it('アプリの版を付ける', async () => {
		await client.GET('/api/v1/auth/me');
		const request = fetchMock.mock.calls[0][0] as Request;
		expect(request.headers.get('X-App-Version')).toBe('1.2.3');
	});

	it('版が古いと断られたら、更新を促す画面に替える', async () => {
		fetchMock.mockResolvedValue(
			Response.json({ error: { code: 'app_update_required', message: '' } }, { status: 426 })
		);
		await client.GET('/api/v1/auth/me');
		expect(appUpdate.required).toBe(true);
	});

	it('ほかのエラーでは替えない', async () => {
		fetchMock.mockResolvedValue(
			Response.json({ error: { code: 'internal_error', message: '' } }, { status: 500 })
		);
		await client.GET('/api/v1/auth/me');
		expect(appUpdate.required).toBe(false);
	});

	it('トークンが無ければ付けない', async () => {
		token.value = null;
		await client.GET('/api/v1/auth/me');
		const request = fetchMock.mock.calls[0][0] as Request;
		expect(request.headers.has('Authorization')).toBe(false);
	});
});

describe('apiFetch (アプリ)', () => {
	it('client と同じ呼び先とトークンで呼ぶ', async () => {
		await apiFetch('/api/v1/records/export', { headers: { Accept: 'text/csv' } });
		const request = fetchMock.mock.calls[0][0] as Request;
		expect(request.url).toBe('https://bp.example.test/api/v1/records/export');
		expect(request.headers.get('Authorization')).toBe('Bearer tok');
		expect(request.headers.get('Accept')).toBe('text/csv');
		expect(request.headers.get('X-App-Version')).toBe('1.2.3');
	});

	it('版が古いと断られたら、更新を促す画面に替える', async () => {
		fetchMock.mockResolvedValue(
			Response.json({ error: { code: 'app_update_required', message: '' } }, { status: 426 })
		);
		await apiFetch('/api/v1/records/export');
		expect(appUpdate.required).toBe(true);
	});
});
