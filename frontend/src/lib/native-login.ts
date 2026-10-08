import { registerPlugin } from '@capacitor/core';

/** アプリの中の各社のログイン (`mobile/plugins/native-login`、docs/mobile-app.md)。 */
interface NativeLoginPlugin {
	signInWithGoogle(options: {
		clientId: string;
		serverClientId: string;
	}): Promise<{ serverAuthCode: string }>;
	signInWithLine(options: {
		channelId: string;
		nonce: string;
	}): Promise<{ idToken: string; accessToken: string }>;
	signInWithApple(options: { nonce: string }): Promise<AppleAuthorization>;
}

/** Sign in with Apple の結果。名前は最初の承認のときだけ入る。 */
export interface AppleAuthorization {
	authorizationCode: string;
	givenName?: string;
	familyName?: string;
}

const NativeLogin = registerPlugin<NativeLoginPlugin>('NativeLogin');

/** iOS 用の Google のクライアント。`mobile/ios/App/App/Info.plist` の URL スキームも、この ID を逆さにしたもの。 */
const GOOGLE_IOS_CLIENT_ID =
	'838577859357-v16smj6g156ajsc23d2qaeuf917b9ths.apps.googleusercontent.com';

/** 認可コードの宛先 (つなぐ先のサーバーのウェブ用のクライアント)。`VITE_APP_GOOGLE_SERVER_CLIENT_ID` は、
 * 開発用のサーバーにつなぐときの差し替え。 */
const GOOGLE_SERVER_CLIENT_ID =
	import.meta.env.VITE_APP_GOOGLE_SERVER_CLIENT_ID ||
	'838577859357-b81ccp62mtdv0ta12ldot1mjp260mr6h.apps.googleusercontent.com';

/** 利用者が画面を閉じたときの失敗 (プラグインの `CANCELED`)。 */
export function isSignInCanceled(error: unknown): boolean {
	return (error as { code?: unknown } | null)?.code === 'CANCELED';
}

/** Google でログインし、サーバーへ渡す認可コードを返す。閉じられたら `isSignInCanceled` に当たる失敗を投げる。 */
export async function googleServerAuthCode(): Promise<string> {
	const { serverAuthCode } = await NativeLogin.signInWithGoogle({
		clientId: GOOGLE_IOS_CLIENT_ID,
		serverClientId: GOOGLE_SERVER_CLIENT_ID
	});
	return serverAuthCode;
}

/** つなぐ先のサーバーの LINE ログインのチャネル。`VITE_APP_LINE_CHANNEL_ID` は、開発用のサーバーにつなぐときの差し替え。 */
const LINE_CHANNEL_ID = import.meta.env.VITE_APP_LINE_CHANNEL_ID || '2011671351';

/** LINE でログインし、ID トークンとアクセストークンを返す。`nonce` はサーバーが発行したもの。
 * 閉じられたら `isSignInCanceled` に当たる失敗を投げる。 */
export function lineTokens(nonce: string): Promise<{ idToken: string; accessToken: string }> {
	return NativeLogin.signInWithLine({ channelId: LINE_CHANNEL_ID, nonce });
}

/** Sign in with Apple で認可コードを得る。`nonce` はサーバーが発行したもの。
 * 閉じられたら `isSignInCanceled` に当たる失敗を投げる。 */
export function appleAuthorization(nonce: string): Promise<AppleAuthorization> {
	return NativeLogin.signInWithApple({ nonce });
}
