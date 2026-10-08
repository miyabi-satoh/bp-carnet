// サーバーが応えないまま、読み込み中から抜けられなくならないように、問い合わせを打ち切る。
import { afterAll, afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

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

import { apiFetch, client } from './client';

/** 打ち切られるまで応えない `fetch`。 */
function hang(input: RequestInfo | URL): Promise<Response> {
	const signal = (input as Request).signal;
	return new Promise((_, reject) => {
		signal.addEventListener('abort', () => reject(signal.reason));
	});
}

beforeEach(() => {
	vi.useFakeTimers();
	fetchMock.mockReset();
	fetchMock.mockImplementation(hang);
});

afterEach(() => {
	vi.useRealTimers();
});

afterAll(() => {
	vi.unstubAllGlobals();
});

describe('問い合わせの時間の上限', () => {
	it('30秒応えなければ打ち切る', async () => {
		const settled = vi.fn();
		const pending = client.GET('/api/v1/settings').then(settled, settled);

		await vi.advanceTimersByTimeAsync(29_999);
		expect(settled).not.toHaveBeenCalled();
		await vi.advanceTimersByTimeAsync(1);
		await pending;
		expect(settled).toHaveBeenCalledOnce();
	});

	it('`apiFetch` も打ち切る', async () => {
		const pending = apiFetch('/api/v1/records/export');
		const rejected = expect(pending).rejects.toThrow();
		await vi.advanceTimersByTimeAsync(30_000);
		await rejected;
	});

	it('写真の読み取りは、サーバーが Gemini を待つ90秒より長く待つ', async () => {
		const settled = vi.fn();
		// 写真は FormData で送る (呼び出しは src/lib/ocr.ts)。型は multipart を JSON の形で持つため合わない。
		// @ts-expect-error -- 上記の理由による。
		const pending = client.POST('/api/v1/ocr', { body: new FormData() }).then(settled, settled);

		await vi.advanceTimersByTimeAsync(90_000);
		expect(settled).not.toHaveBeenCalled();
		await vi.advanceTimersByTimeAsync(30_000);
		await pending;
		expect(settled).toHaveBeenCalledOnce();
	});

	it('応えが返れば、その後に時間が来ても何もしない', async () => {
		fetchMock.mockResolvedValue(Response.json({}));
		const { response } = await client.GET('/api/v1/settings');
		expect(response.ok).toBe(true);
		expect(vi.getTimerCount()).toBe(0);
	});
});
