import { page } from '$app/state';
import { client } from '$lib/api/client';
import {
	type ApiResult,
	ensureOkRequest,
	GENERIC_ERROR_MESSAGE,
	unwrapRequest
} from '$lib/api/errors';
import type { components } from '$lib/api/schema';
import { clearAppToken, isNativeApp, saveAppToken } from '$lib/native-app';
import {
	appleAuthorization,
	googleServerAuthCode,
	isSignInCanceled,
	lineTokens
} from '$lib/native-login';
import * as m from '$lib/paraglide/messages.js';

/**
 * 未ログインでもアクセスできるページの route id。`+layout.ts` と `+layout.svelte` で共有する。
 *
 * `url.pathname` ではなく `route.id` で判定する: パスのローカライズ (Paraglide の `/ja/login` 等) や
 * base path があっても route id は変わらないため。
 */
export const PUBLIC_ROUTES: readonly string[] = [
	// メールアドレスの変更の確定は、リンクを開いたブラウザでログインしていなくてもできる。
	'/change-email',
	// 使い方は、ログインできないときにも読めるようにする。
	'/help',
	'/help/[slug]',
	// 使っている部品のライセンスは、URL を直に開けばアカウントを持たない人も見られるようにする。
	'/licenses',
	'/login',
	// 利用規約・プライバシーポリシーは、登録する前に読めるようにする。
	'/privacy',
	// 料金と購入の条件は、amiiby.com の特定商取引法の表記から指すので、登録前にも読めるようにする。
	'/pricing',
	'/reset-password',
	'/signup',
	'/terms',
	'/verify-email'
];

/** ログインを確かめられなかった理由。`offline` はサーバーに届かなかった、`server` はサーバーがエラーを返した。
 * 例外 (`error()`) にしないのは、ルートの layout の load が失敗すると、SvelteKit がエラーページのために
 * 同じ load を走らせ直し、そこでも失敗して `+error.svelte` ではなく素の静的なページを出すため。
 * 代わりにルートの layout が本文の代わりに知らせの画面を出す (docs/mobile-app.md)。 */
export type LoadFailure = 'offline' | 'server';

export function isPublicRoute(routeId: string | null): boolean {
	return routeId !== null && PUBLIC_ROUTES.includes(routeId);
}

/** 画面の見出しに使う表示名。Google ログインで取得したプロフィールの名前があればそれを使い、
 * 無ければユーザーID。ログイン中ユーザー (`MeResponse`) と管理画面のユーザー
 * (`AdminUserResponse`) の両方に使う。 */
export function userDisplayName(
	user: { displayName?: string | null; username: string } | null | undefined
): string {
	if (!user) return '';
	return user.displayName ?? user.username;
}

/** アバターに出す1文字。表示名 (無ければユーザーID) の先頭 (絵文字・サロゲートペアを
 * 割らないようコードポイント単位で取る)。ヘッダーのユーザーメニューで使う。 */
export function userInitial(user: Parameters<typeof userDisplayName>[0]): string {
	return [...userDisplayName(user)][0] ?? '';
}

/** ログイン中ユーザーの個人設定のタイムゾーン。ログインが要る画面でだけ呼ぶ (`+layout.ts` が `user` を
 * 読み込み済み)。 */
export function userTimeZone(): string {
	const user: components['schemas']['MeResponse'] | null = page.data.user;
	if (!user) throw new Error('userTimeZone: called outside a logged-in page');
	return user.timezone;
}

export type AuthProviders = components['schemas']['AuthProvidersResponse'];

/** 表示を決めるだけの問い合わせ (βの表示・問い合わせ先) を待つ上限。間に合わなければ
 * 無難な側の値で出す。 */
const LOGIN_SCREEN_TIMEOUT_MS = 3000;

/** 使えるログイン方式の問い合わせを待つ上限。これが分からないとログイン・サインアップの画面を出せないため、
 * 止まっている本番サーバーが起きるまで (実測で約3秒) は待つ。過ぎたら届かなかった
 * 扱いにして、「もう一度試す」の画面を出す。 */
const PROVIDERS_TIMEOUT_MS = 10000;

/** `promise` が `ms` 以内に返らなければ、その時点で `fallback()` を返す。 */
async function withTimeout<T>(
	promise: Promise<T>,
	fallback: () => T,
	ms = LOGIN_SCREEN_TIMEOUT_MS
): Promise<T> {
	let timer: ReturnType<typeof setTimeout> | undefined;
	const timeout = new Promise<T>((resolve) => {
		timer = setTimeout(() => resolve(fallback()), ms);
	});
	try {
		return await Promise.race([promise, timeout]);
	} finally {
		clearTimeout(timer);
	}
}

/** 画面を出すのに要る問い合わせの結果。届かなかった・エラーだったときは、画面を出さずに
 * 「もう一度試す」を出す (docs/mobile-app.md)。どれも使えない扱いで出すと、LINE・Google のボタンが
 * 黙って消え、通信のせいだと分からないため。 */
export type LoadResult<T> = { ok: true; data: T } | { ok: false; failure: LoadFailure };

/** `GET /api/v1/auth/providers` — 未ログインで叩ける公開エンドポイント。画面はこれを見て
 * 各社のログインのボタンの表示可否を判断する。 */
export function fetchAuthProviders(): Promise<LoadResult<AuthProviders>> {
	return withTimeout(requestAuthProviders(), timedOut, PROVIDERS_TIMEOUT_MS);
}

/** 締め切りまでに返らなかった。止まっているのかつながらないのかは分からないので、つながらない側で知らせる。 */
function timedOut(): { ok: false; failure: LoadFailure } {
	return { ok: false, failure: 'offline' };
}

async function requestAuthProviders(): Promise<LoadResult<AuthProviders>> {
	try {
		const { data, response } = await client.GET('/api/v1/auth/providers');
		if (!response.ok || !data) return { ok: false, failure: 'server' };
		return { ok: true, data };
	} catch {
		return { ok: false, failure: 'offline' };
	}
}

/** 「オープンβテスト中」の表示を出す構成か。ログイン済みの画面のヘッダーが使う (ログイン・サインアップ画面は
 * それぞれの問い合わせの応答を使う)。問い合わせに失敗したら出さない側にする: 表示が無くても困らないため。 */
export async function fetchBetaNotice(): Promise<boolean> {
	const result = await withTimeout(requestAuthProviders(), timedOut);
	return result.ok && result.data.betaNotice;
}

/** 問い合わせ先の URL を問い合わせる。問い合わせに失敗したり、締め切り (`LOGIN_SCREEN_TIMEOUT_MS`) までに
 * 返らなかったりしたら `null` (URL が分からないのでリンクを出さない)。 */
export async function fetchContactUrl(): Promise<string | null> {
	const result = await withTimeout(requestAuthProviders(), timedOut);
	return result.ok ? result.data.contactUrl : null;
}

/** ログインの結果。猶予期間中の削除予約を取り消したかを返す (ホーム画面で知らせるため)。 */
export type LoginResult = ApiResult<{ deletionCancelled: boolean }>;

/** ID/PW でログインする。 */
export async function login(username: string, password: string): Promise<LoginResult> {
	if (isNativeApp) {
		return completeAppLogin(
			await unwrapRequest(client.POST('/api/v1/app/auth/login', { body: { username, password } }))
		);
	}
	const result = await unwrapRequest(
		client.POST('/api/v1/auth/login', { body: { username, password } })
	);
	return result.ok ? { ok: true, deletionCancelled: result.data.deletionCancelled } : result;
}

/** アプリで Google でログインする (docs/mobile-app.md)。画面を閉じられたら `null`。 */
export function loginWithGoogle(): Promise<LoginResult | null> {
	return nativeLogin(googleServerAuthCode, (serverAuthCode) =>
		unwrapRequest(client.POST('/api/v1/app/auth/google', { body: { serverAuthCode } }))
	);
}

/** アプリで LINE でログインする (docs/mobile-app.md)。画面を閉じられたら `null`。 */
export async function loginWithLine(): Promise<LoginResult | null> {
	const issued = await issueAppNonce();
	if (!issued.ok) return issued;
	const { nonce } = issued.data;
	return nativeLogin(
		() => lineTokens(nonce),
		(tokens) => unwrapRequest(client.POST('/api/v1/app/auth/line', { body: { ...tokens, nonce } }))
	);
}

/** アプリで Apple でログインする (docs/mobile-app.md)。画面を閉じられたら `null`。 */
export async function loginWithApple(): Promise<LoginResult | null> {
	const issued = await issueAppNonce();
	if (!issued.ok) return issued;
	const { nonce } = issued.data;
	return nativeLogin(
		() => appleAuthorization(nonce),
		(authorization) =>
			unwrapRequest(client.POST('/api/v1/app/auth/apple', { body: { ...authorization, nonce } }))
	);
}

/** LINE・Apple の SDK に渡す nonce をサーバーに発行してもらう。ID トークンの使い回しを防ぐため。 */
function issueAppNonce() {
	return unwrapRequest(client.POST('/api/v1/app/auth/nonce'));
}

/** 各社の SDK でログインし (`signIn`)、得た値をアプリ用の入口へ送る (`post`)。画面を閉じられたら `null`。 */
async function nativeLogin<T>(
	signIn: () => Promise<T>,
	post: (value: T) => Promise<ApiResult<{ data: components['schemas']['AppLoginResponse'] }>>
): Promise<LoginResult | null> {
	let value: T;
	try {
		value = await signIn();
	} catch (error) {
		if (isSignInCanceled(error)) return null;
		return { ok: false, message: GENERIC_ERROR_MESSAGE() };
	}
	return completeAppLogin(await post(value));
}

/** アプリのログインの入口が返したトークンをキーチェーンにしまう。 */
async function completeAppLogin(
	result: ApiResult<{ data: components['schemas']['AppLoginResponse'] }>
): Promise<LoginResult> {
	if (!result.ok) return result;
	const { token } = result.data;
	try {
		await saveAppToken(token);
	} catch {
		// しまえなければ次に起動したときはログインしていないので、発行されたトークンをサーバーに残さない。
		await ensureOkRequest(
			client.POST('/api/v1/app/auth/logout', {
				headers: { Authorization: `Bearer ${token}` }
			})
		);
		return { ok: false, message: GENERIC_ERROR_MESSAGE() };
	}
	return { ok: true, deletionCancelled: result.data.user.deletionCancelled };
}

/** サインアップを申し込む。登録の有無に関わらず同じ応答が返る (違いは届くメールの中身だけ)。 */
export function requestSignup(email: string): Promise<ApiResult> {
	return ensureOkRequest(client.POST('/api/v1/auth/signup', { body: { email } }));
}

/** メールでのパスワードの再設定を申し込む。アカウントの有無に関わらず同じ応答が返る。 */
export function requestPasswordReset(email: string): Promise<ApiResult> {
	return ensureOkRequest(client.POST('/api/v1/auth/password-reset', { body: { email } }));
}

/** 再設定のリンクのトークンで新しいパスワードを設定する。ログインはしない (ログイン画面から入り直す)。 */
export function completePasswordReset(token: string, password: string): Promise<ApiResult> {
	return ensureOkRequest(
		client.POST('/api/v1/auth/password-reset/complete', { body: { token, password } })
	);
}

/** メールのリンク (確認・再設定) がまだ使えるかを、使用済みにせずに確かめる。使えないと分かったときだけ
 * `false` を返す。通信の失敗などではフォームを出し、送ったときの応答で知らせる。 */
export async function isEmailLinkUsable(
	purpose: 'signup' | 'password-reset',
	token: string
): Promise<boolean> {
	const result = await ensureOkRequest(
		purpose === 'signup'
			? client.POST('/api/v1/auth/signup/check', { body: { token } })
			: client.POST('/api/v1/auth/password-reset/check', { body: { token } })
	);
	return result.ok || result.code !== 'invalid_email_token';
}

/** 確認リンクのトークンでパスワードを設定する。成功するとログイン済みになる。 */
export function completeSignup(token: string, password: string): Promise<ApiResult> {
	return ensureOkRequest(
		client.POST('/api/v1/auth/signup/complete', { body: { token, password } })
	);
}

/** ログアウトする。ヘッダーのユーザーメニューと設定画面の両方から呼ぶため共通化した。
 * サーバー側でセッションを破棄できていない状態のままログイン画面へ遷移すると、共有端末で
 * 「ログアウトしたつもり」で有効なセッションが残ってしまうため、失敗 (`false`) を返した
 * 呼び出し元は遷移しないこと。 */
export async function logout(): Promise<boolean> {
	if (isNativeApp) {
		if (!(await ensureOkRequest(client.POST('/api/v1/app/auth/logout'))).ok) return false;
		// サーバーのトークンはもう効かないので、キーチェーンから消せなくてもログアウトは済んでいる。
		try {
			await clearAppToken();
		} catch {
			// 残ったトークンは、次に起動したとき 401 になってログイン画面へ送られる。
		}
		return true;
	}
	return (await ensureOkRequest(client.POST('/api/v1/auth/logout'))).ok;
}

/** アカウントの削除を予約する。即時には削除されず、猶予期間中にログインし直すと取り消される
 * (docs/authentication.md)。確認文字列の入力は画面側で確かめる (誤操作を防ぐための UI)。 */
export function scheduleAccountDeletion(): Promise<ApiResult> {
	return ensureOkRequest(client.POST('/api/v1/account/deletion'));
}

/** パスワードを変更する。現在のパスワードの照合・ポリシーの判定はどちらもサーバーが行う
 * (docs/authentication.md)。 */
export function changePassword(currentPassword: string, newPassword: string): Promise<ApiResult> {
	return ensureOkRequest(
		client.PUT('/api/v1/account/password', { body: { currentPassword, newPassword } })
	);
}

/** メールアドレスの変更を申し込む。新しいアドレスに確定のリンクが届く (docs/authentication.md)。
 * `currentPassword` はパスワードを持つアカウントだけ送る。 */
export function requestEmailChange(
	newEmail: string,
	currentPassword: string | null
): Promise<ApiResult> {
	return ensureOkRequest(
		client.POST('/api/v1/account/email', { body: { newEmail, currentPassword } })
	);
}

/** メールアドレスの変更のリンクがまだ使えれば、変更先のアドレスを返す。 */
export function emailChangeTarget(
	token: string
): Promise<ApiResult<{ data: { newEmail: string } }>> {
	return unwrapRequest(client.POST('/api/v1/auth/email-change/check', { body: { token } }));
}

/** メールアドレスの変更を確定する。ログインしていなくてもよい。 */
export function completeEmailChange(token: string): Promise<ApiResult> {
	return ensureOkRequest(client.POST('/api/v1/auth/email-change/complete', { body: { token } }));
}

/** 連携している外部アカウントの提供元 (`/auth/me` の `linkedProviders` の要素)。 */
export type LinkedProvider = components['schemas']['Provider'];

/** 提供元を並べる順 (ログイン画面のボタン・設定・管理画面で共通)。 */
export const LINKED_PROVIDERS = [
	'line',
	'google',
	'apple'
] as const satisfies readonly LinkedProvider[];

/** 提供元の表示名。 */
export function linkedProviderLabel(provider: LinkedProvider): string {
	switch (provider) {
		case 'line':
			return m.common_provider_line();
		case 'google':
			return m.common_provider_google();
		case 'apple':
			return m.common_provider_apple();
	}
}

/** 提供元の表示名を並べる (「LINE・Google」など)。 */
export function linkedProviderLabels(providers: readonly LinkedProvider[]): string {
	return providers.map(linkedProviderLabel).join(m.common_list_separator());
}

/** 連携を解除する。解除できるのは、ほかにログイン方法 (パスワード・別の連携) が残るときだけで、
 * 残らなければサーバーが 409 で断る (docs/authentication.md)。LINE・Apple 側の取り消しもサーバーが行う。 */
export function unlinkIdentity(provider: LinkedProvider): Promise<ApiResult> {
	return ensureOkRequest(
		client.DELETE('/api/v1/account/identities/{provider}', { params: { path: { provider } } })
	);
}

/** ログイン方法を1つ減らしても、ログインできる方法が残るか。表示の判断に使うだけで、
 * 本当の判定はサーバーが行う。 */
export function canUnlinkIdentity(user: {
	passwordUsable: boolean;
	linkedProviders: readonly LinkedProvider[];
}): boolean {
	return user.passwordUsable || user.linkedProviders.length >= 2;
}
