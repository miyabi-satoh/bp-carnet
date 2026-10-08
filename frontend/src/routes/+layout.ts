import { redirect } from '@sveltejs/kit';
import { browser } from '$app/environment';
import { resolve } from '$app/paths';
import { client } from '$lib/api/client';
import { isPublicRoute, type LoadFailure } from '$lib/auth';
import { browserTimeZone, saveDetectedTimezone } from '$lib/settings';
import { getLocale } from '$lib/paraglide/runtime.js';
import type { LayoutLoad } from './$types';

export const ssr = false;

// app.html の `lang` は既定の言語 (ja) の固定値なので、表示言語に合わせ直す。言語を変えるとページを読み直すため、読み込み時の1回でよい。
// ビルド時に Node でこのファイルが読まれることがあるので、ブラウザでだけ行う。
if (browser) document.documentElement.lang = getLocale();

/** 同期に失敗した「ユーザーとブラウザのタイムゾーン」の組。同じ組は投げ直さない
 * (遷移のたび、リンクへのホバーによる先読みのたびに繰り返さないため)。ユーザーを含めるのは、
 * 同じブラウザで別のユーザーに入り直したときに同期を止めてしまわないため。 */
let failedSync: string | undefined;

/** ログイン中のユーザーを問い合わせる。通信できなければ `undefined`。 */
async function fetchMe() {
	try {
		return await client.GET('/api/v1/auth/me');
	} catch {
		return undefined;
	}
}

// ssr = false のため、これはブラウザでのみ実行される。
// アプリ起動時・全ページ遷移時に `/auth/me` を呼び、未ログインならログイン画面へ誘導する。
// ログイン画面自体とパスワードリセット画面は未ログインでもアクセスできる必要があるため対象外とする。
export const load: LayoutLoad = async ({ route }) => {
	if (isPublicRoute(route.id)) {
		// ログイン済みでログイン画面を開いたら (古い履歴・ブックマーク)、フォームを出さずにホーム画面へ送る。
		// 確かめられなければ、そのままログイン画面を出す。
		if (route.id === '/login') {
			if ((await fetchMe())?.response.ok) redirect(307, resolve('/'));
		}
		return { user: null };
	}

	// 通信できない・5xx のときは、セッションが有効なままかもしれないので、ログイン画面へは送らない。
	const me = await fetchMe();
	if (!me) return { user: null, loadFailure: 'offline' as LoadFailure };
	let { data } = me;
	const { response } = me;

	// 401 のみ未ログインとみなす。
	if (response.status === 401) {
		redirect(307, resolve('/login'));
	}
	if (!response.ok || !data) return { user: null, loadFailure: 'server' as LoadFailure };

	// 自動設定なら、ブラウザのタイムゾーンに揃える。揃えられなくても保存値のまま開く。
	const detected = browserTimeZone();
	const syncKey = `${data.id}:${detected}`;
	if (data.timezoneAuto && detected && detected !== data.timezone && syncKey !== failedSync) {
		const result = await saveDetectedTimezone(detected);
		if (result.ok) {
			data = { ...data, timezone: result.data.timezone, timezoneAuto: result.data.timezoneAuto };
		} else {
			failedSync = syncKey;
		}
	}

	return { user: data };
};
