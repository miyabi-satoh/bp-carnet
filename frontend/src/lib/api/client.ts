import createClient from 'openapi-fetch';
import { goto } from '$app/navigation';
import { resolve } from '$app/paths';
import { appUpdate } from '$lib/app-update.svelte';
import { errorCode } from '$lib/api/errors';
import type { paths } from '$lib/api/schema';
import { API_ORIGIN, appToken, appVersion, isNativeApp } from '$lib/native-app';

/** 問い合わせを打ち切るまでの時間。サーバーが応えないまま待ち続け、読み込み中から抜けられなくなるのを防ぐ。 */
const REQUEST_TIMEOUT_MS = 30_000;
/** 写真の読み取りは、サーバーが Gemini を待つ上限 (90秒、src/ocr.rs) より長く待つ。 */
const OCR_TIMEOUT_MS = 120_000;
const OCR_PATH = '/api/v1/ocr';

/** `timeoutMs` で打ち切る `fetch`。打ち切ると通信エラーと同じく reject する (呼び出し側は汎用の文言を出す)。
 * ADR: `AbortSignal.any` は使わない。アプリは iOS 16.4 から動かすため (Safari 17.4 から)。 */
async function fetchWithTimeout(input: Request, timeoutMs: number): Promise<Response> {
	const controller = new AbortController();
	const abort = () => controller.abort();
	const timer = setTimeout(abort, timeoutMs);
	if (input.signal.aborted) abort();
	else input.signal.addEventListener('abort', abort, { once: true });
	try {
		return await fetch(new Request(input, { signal: controller.signal }));
	} finally {
		clearTimeout(timer);
		input.signal.removeEventListener('abort', abort);
	}
}

function timeoutFor(path: string): number {
	return path === OCR_PATH ? OCR_TIMEOUT_MS : REQUEST_TIMEOUT_MS;
}

// openapi.json のパスは `/api/v1/...` を含む (backend 側で nest した prefix がそのまま
// ドキュメントに載る) ため、baseUrl はオリジンだけで良い (ウェブは空)。
export const client = createClient<paths>({
	baseUrl: API_ORIGIN,
	fetch: (request) => fetchWithTimeout(request, timeoutFor(new URL(request.url).pathname))
});

// アプリは Cookie を使えないので、トークンを付けて送る。版も付けて、古い版をサーバーが断れるようにする
// (docs/mobile-app.md)。
if (isNativeApp) {
	client.use({
		async onRequest({ request }) {
			await addAppHeaders(request.headers);
			return request;
		},
		async onResponse({ response }) {
			await noticeUpdateRequired(response);
			return response;
		}
	});
}

// 使っている途中でログインが切れたら (別の端末でパスワードを変えた・凍結された・長く使わなかった)、
// ログイン画面へ送る。画面を開くときの確認 (`+layout.ts`) だけで見ていると、同じ画面の操作では
// 「ログインが必要です」が出るだけで、ログインへ戻る道が無いため。
client.use({
	async onResponse({ request, response }) {
		await leaveIfSessionEnded(new URL(request.url).pathname, response);
		return response;
	}
});

/** ログインの確認 (`/auth/me`) は `+layout.ts` が自分で扱う (ログイン画面でも呼ぶので、ここで送ると回り続ける)。 */
const SESSION_CHECK_PATH = '/api/v1/auth/me';

async function leaveIfSessionEnded(path: string, response: Response): Promise<void> {
	if (response.status !== 401 || path === SESSION_CHECK_PATH) return;
	// 401 には、パスワードの誤りなど、ログインの切れ以外のコードもある (src/error.rs)。
	const body: unknown = await response
		.clone()
		.json()
		.catch(() => null);
	if (errorCode(body) !== 'unauthorized') return;
	// 同じ画面の問い合わせが揃って切れていたときに、ログイン画面への移動を重ねない。
	leaving ??= goto(resolve('/login'), { replaceState: true }).finally(() => (leaving = undefined));
	await leaving;
}

/** 進んでいるログイン画面への移動。 */
let leaving: Promise<void> | undefined;

async function addAppHeaders(headers: Headers): Promise<void> {
	const version = await appVersion();
	if (version) headers.set('X-App-Version', version);
	// 呼び出し側が付けたもの (保存し損ねたトークンの失効) を優先する。
	if (headers.has('Authorization')) return;
	const token = await appToken();
	if (token) headers.set('Authorization', `Bearer ${token}`);
}

/** 版が古いと断られたら、更新を促す画面に替える。 */
async function noticeUpdateRequired(response: Response): Promise<void> {
	if (response.status !== 426) return;
	const body: unknown = await response
		.clone()
		.json()
		.catch(() => null);
	if (errorCode(body) === 'app_update_required') appUpdate.required = true;
}

/** `client` を使えない呼び出し (JSON 以外の応答) 用の `fetch`。呼び先とログインの証しを `client` とそろえる。 */
export async function apiFetch(
	path: `/api/v1/${string}`,
	init: RequestInit = {}
): Promise<Response> {
	const headers = new Headers(init.headers);
	if (isNativeApp) await addAppHeaders(headers);
	const request = new Request(`${API_ORIGIN}${path}`, {
		credentials: 'same-origin',
		...init,
		headers
	});
	const response = await fetchWithTimeout(request, timeoutFor(path));
	if (isNativeApp) await noticeUpdateRequired(response);
	await leaveIfSessionEnded(path, response);
	return response;
}
