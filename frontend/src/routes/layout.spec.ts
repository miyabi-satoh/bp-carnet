// ルートの layout の load: ログインを確かめられないときは、例外にせず理由を返す (docs/mobile-app.md)。
import { isRedirect } from '@sveltejs/kit';
import { beforeEach, describe, expect, it, vi } from 'vitest';

vi.mock('$app/paths', () => ({ resolve: (path: string) => path }));
vi.mock('$lib/api/client', () => ({ client: { GET: vi.fn() } }));
vi.mock('$lib/settings', () => ({
	browserTimeZone: () => undefined,
	saveDetectedTimezone: vi.fn()
}));

import { client } from '$lib/api/client';
import { load } from './+layout';
import { respond } from '$lib/api/client.test-helpers';

// eslint-disable-next-line @typescript-eslint/no-explicit-any
const run = async (routeId: string) => load({ route: { id: routeId } } as any);

beforeEach(() => {
	vi.clearAllMocks();
});

describe('layout の load', () => {
	it('サーバーに届かなければ offline を返す', async () => {
		vi.mocked(client.GET).mockRejectedValue(new TypeError('Load failed'));
		expect(await run('/')).toEqual({ user: null, loadFailure: 'offline' });
	});

	it('サーバーがエラーを返せば server を返す', async () => {
		vi.mocked(client.GET).mockReturnValue(respond(503));
		expect(await run('/')).toEqual({ user: null, loadFailure: 'server' });
	});

	it('401 ならログイン画面へ送る', async () => {
		vi.mocked(client.GET).mockReturnValue(respond(401));
		const thrown = await run('/').catch((e: unknown) => e);
		expect(isRedirect(thrown) && thrown.location).toBe('/login');
	});

	it('ログインできていればユーザーを返す', async () => {
		const user = { id: 1, timezoneAuto: false, timezone: 'Asia/Tokyo' };
		vi.mocked(client.GET).mockReturnValue(respond(200, user));
		expect(await run('/')).toEqual({ user });
	});
});
