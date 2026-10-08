// アプリ (Capacitor) の中で動いているときの分岐 (docs/mobile-app.md)。
import { beforeEach, describe, expect, it, vi } from 'vitest';

vi.mock('$app/state', () => ({ page: { data: {} } }));
vi.mock('$lib/api/client', () => ({ client: { GET: vi.fn(), POST: vi.fn() } }));
vi.mock('$lib/native-app', () => ({
	isNativeApp: true,
	saveAppToken: vi.fn(),
	clearAppToken: vi.fn()
}));
vi.mock('$lib/native-login', async (importOriginal) => ({
	...(await importOriginal<typeof import('$lib/native-login')>()),
	appleAuthorization: vi.fn(),
	googleServerAuthCode: vi.fn(),
	lineTokens: vi.fn()
}));

import { client } from '$lib/api/client';
import { clearAppToken, saveAppToken } from '$lib/native-app';
import { appleAuthorization, googleServerAuthCode, lineTokens } from '$lib/native-login';
import { login, loginWithApple, loginWithGoogle, loginWithLine, logout } from './auth';
import { fetchOcrStatus } from './ocr';
import { respond } from '$lib/api/client.test-helpers';

beforeEach(() => {
	vi.clearAllMocks();
});

describe('login (アプリ)', () => {
	it('アプリ用の入口を呼び、受け取ったトークンを保存する', async () => {
		vi.mocked(client.POST).mockReturnValue(
			respond(200, { token: 'tok', user: { deletionCancelled: true } })
		);
		expect(await login('alice', 'password')).toEqual({ ok: true, deletionCancelled: true });
		expect(client.POST).toHaveBeenCalledWith('/api/v1/app/auth/login', {
			body: { username: 'alice', password: 'password' }
		});
		expect(saveAppToken).toHaveBeenCalledWith('tok');
	});

	it('トークンをしまえなければ、そのトークンを失効させて失敗を返す', async () => {
		vi.mocked(client.POST)
			.mockReturnValueOnce(respond(200, { token: 'tok', user: { deletionCancelled: false } }))
			.mockReturnValueOnce(respond(204));
		vi.mocked(saveAppToken).mockRejectedValueOnce(new Error('keychain'));
		expect((await login('alice', 'password')).ok).toBe(false);
		expect(client.POST).toHaveBeenLastCalledWith('/api/v1/app/auth/logout', {
			headers: { Authorization: 'Bearer tok' }
		});
	});

	it('失敗したらトークンを保存しない', async () => {
		vi.mocked(client.POST).mockReturnValue(respond(401, { error: { code: 'unauthorized' } }));
		expect((await login('alice', 'wrong')).ok).toBe(false);
		expect(saveAppToken).not.toHaveBeenCalled();
	});
});

describe('loginWithGoogle', () => {
	it('Google の認可コードをアプリ用の入口に渡し、受け取ったトークンを保存する', async () => {
		vi.mocked(googleServerAuthCode).mockResolvedValue('code');
		vi.mocked(client.POST).mockReturnValue(
			respond(200, { token: 'tok', user: { deletionCancelled: false } })
		);
		expect(await loginWithGoogle()).toEqual({ ok: true, deletionCancelled: false });
		expect(client.POST).toHaveBeenCalledWith('/api/v1/app/auth/google', {
			body: { serverAuthCode: 'code' }
		});
		expect(saveAppToken).toHaveBeenCalledWith('tok');
	});

	it('Google の画面を閉じたら何もせず null を返す', async () => {
		vi.mocked(googleServerAuthCode).mockRejectedValue(
			Object.assign(new Error('キャンセルされました'), { code: 'CANCELED' })
		);
		expect(await loginWithGoogle()).toBeNull();
		expect(client.POST).not.toHaveBeenCalled();
	});

	it('Google でのログインが失敗したら、サーバーを呼ばずに失敗を返す', async () => {
		vi.mocked(googleServerAuthCode).mockRejectedValue(new Error('network'));
		expect((await loginWithGoogle())?.ok).toBe(false);
		expect(client.POST).not.toHaveBeenCalled();
	});
});

describe('loginWithLine', () => {
	it('サーバーの nonce を SDK に渡し、得たトークンと nonce をアプリ用の入口に渡す', async () => {
		vi.mocked(client.POST)
			.mockReturnValueOnce(respond(200, { nonce: 'n1' }))
			.mockReturnValueOnce(respond(200, { token: 'tok', user: { deletionCancelled: false } }));
		vi.mocked(lineTokens).mockResolvedValue({ idToken: 'id', accessToken: 'at' });
		expect(await loginWithLine()).toEqual({ ok: true, deletionCancelled: false });
		expect(lineTokens).toHaveBeenCalledWith('n1');
		expect(client.POST).toHaveBeenLastCalledWith('/api/v1/app/auth/line', {
			body: { idToken: 'id', accessToken: 'at', nonce: 'n1' }
		});
		expect(saveAppToken).toHaveBeenCalledWith('tok');
	});

	it('nonce を受け取れなければ、LINE の画面を出さずに失敗を返す', async () => {
		vi.mocked(client.POST).mockReturnValue(respond(429, { error: { code: 'too_many_requests' } }));
		expect((await loginWithLine())?.ok).toBe(false);
		expect(lineTokens).not.toHaveBeenCalled();
	});

	it('LINE の画面を閉じたら null を返す', async () => {
		vi.mocked(client.POST).mockReturnValue(respond(200, { nonce: 'n1' }));
		vi.mocked(lineTokens).mockRejectedValue(Object.assign(new Error('x'), { code: 'CANCELED' }));
		expect(await loginWithLine()).toBeNull();
		expect(client.POST).toHaveBeenCalledTimes(1);
	});
});

describe('loginWithApple', () => {
	it('サーバーの nonce を Sign in with Apple に渡し、得た認可コード・名前と nonce をアプリ用の入口に渡す', async () => {
		vi.mocked(client.POST)
			.mockReturnValueOnce(respond(200, { nonce: 'n1' }))
			.mockReturnValueOnce(respond(200, { token: 'tok', user: { deletionCancelled: false } }));
		vi.mocked(appleAuthorization).mockResolvedValue({
			authorizationCode: 'code',
			givenName: '花子',
			familyName: '山田'
		});
		expect(await loginWithApple()).toEqual({ ok: true, deletionCancelled: false });
		expect(appleAuthorization).toHaveBeenCalledWith('n1');
		expect(client.POST).toHaveBeenLastCalledWith('/api/v1/app/auth/apple', {
			body: { authorizationCode: 'code', givenName: '花子', familyName: '山田', nonce: 'n1' }
		});
		expect(saveAppToken).toHaveBeenCalledWith('tok');
	});

	it('nonce を受け取れなければ、Apple の画面を出さずに失敗を返す', async () => {
		vi.mocked(client.POST).mockReturnValue(respond(429, { error: { code: 'too_many_requests' } }));
		expect((await loginWithApple())?.ok).toBe(false);
		expect(appleAuthorization).not.toHaveBeenCalled();
	});

	it('Apple の画面を閉じたら null を返す', async () => {
		vi.mocked(client.POST).mockReturnValue(respond(200, { nonce: 'n1' }));
		vi.mocked(appleAuthorization).mockRejectedValue(
			Object.assign(new Error('x'), { code: 'CANCELED' })
		);
		expect(await loginWithApple()).toBeNull();
		expect(client.POST).toHaveBeenCalledTimes(1);
	});
});

describe('logout (アプリ)', () => {
	it('アプリ用の入口を呼び、トークンを消す', async () => {
		vi.mocked(client.POST).mockReturnValue(respond(204));
		expect(await logout()).toBe(true);
		expect(client.POST).toHaveBeenCalledWith('/api/v1/app/auth/logout');
		expect(clearAppToken).toHaveBeenCalled();
	});

	it('キーチェーンから消せなくても、サーバーで失効していればログアウトできる', async () => {
		vi.mocked(client.POST).mockReturnValue(respond(204));
		vi.mocked(clearAppToken).mockRejectedValueOnce(new Error('keychain'));
		expect(await logout()).toBe(true);
	});

	// サーバーのトークンが生きたまま手元だけ消すと、ログアウトしたつもりで有効なトークンが残る。
	it('失敗したらトークンを消さない', async () => {
		vi.mocked(client.POST).mockReturnValue(respond(500));
		expect(await logout()).toBe(false);
		expect(clearAppToken).not.toHaveBeenCalled();
	});
});

describe('fetchOcrStatus (アプリ)', () => {
	it('買い足せるかは、Stripe でなくアプリ内課金の値で決める', async () => {
		vi.mocked(client.GET).mockReturnValue(
			respond(200, {
				enabled: true,
				quotas: [],
				quotaLow: true,
				topupAvailable: true,
				appStoreTopupAvailable: false
			})
		);
		expect((await fetchOcrStatus()).topupAvailable).toBe(false);
		vi.mocked(client.GET).mockReturnValue(
			respond(200, {
				enabled: true,
				quotas: [],
				quotaLow: true,
				topupAvailable: false,
				appStoreTopupAvailable: true
			})
		);
		expect((await fetchOcrStatus()).topupAvailable).toBe(true);
	});
});
