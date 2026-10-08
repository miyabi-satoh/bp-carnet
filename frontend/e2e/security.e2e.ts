// CSRF の同一オリジンチェック (docs/authentication.md、src/csrf.rs) と、セッション Cookie の属性。
// ログアウト・ログインで確かめるため、共有の管理者のセッションは使わず使い捨てユーザーで流す。
import type { APIRequestContext } from '@playwright/test';
import { loginStatus } from './api-helpers';
import { test, expect } from './fixtures';

/** テストの接続先とは別のオリジン。 */
const OTHER_ORIGIN = 'https://evil.example';

test('別のオリジンからの GET 以外は 403 になり、同じオリジンや Origin・Referer の無いものは通る', async ({
	managedUser: user,
	baseURL
}) => {
	const logOut = (headers: Record<string, string>) =>
		user.request.post('/api/v1/auth/logout', { headers });
	const meStatus = async () => (await user.request.get('/api/v1/auth/me')).status();

	// 状態を変える操作にログアウトを使う。拒まれたら、ログインしたままのはず。
	expect((await logOut({ Origin: OTHER_ORIGIN })).status()).toBe(403);
	expect(await meStatus()).toBe(200);
	// Origin を送らないブラウザのために、Referer でも比べる。
	expect((await logOut({ Referer: `${OTHER_ORIGIN}/page` })).status()).toBe(403);
	expect(await meStatus()).toBe(200);

	expect((await logOut({ Origin: new URL(baseURL!).origin })).status()).toBe(204);
	expect(await meStatus()).toBe(401);

	// API のコンテキストは Origin・Referer を付けないので、ログインがそのまま通る。
	expect(await loginStatus(user.request, user.username, user.password)).toBe(200);
});

test.describe('セッション Cookie', () => {
	/** ログインしたコンテキストのセッション Cookie。名前は Secure の構成 (https) では `__Host-id`、それ以外は tower-sessions の既定の `id`。 */
	async function sessionCookie(request: APIRequestContext, secure: boolean) {
		const name = secure ? '__Host-id' : 'id';
		const cookie = (await request.storageState()).cookies.find((c) => c.name === name);
		expect(cookie).toBeDefined();
		return cookie!;
	}

	// Secure を付けるかは設定 (`[session] secure_cookie`) で決まり、https で公開する環境だけが付ける。
	test('ログインすると HttpOnly・SameSite=Strict が付き、https の環境でだけ Secure が付く', async ({
		managedUser: user,
		baseURL
	}) => {
		const secure = new URL(baseURL!).protocol === 'https:';
		expect(await sessionCookie(user.request, secure)).toMatchObject({
			httpOnly: true,
			sameSite: 'Strict',
			secure
		});
	});
});
