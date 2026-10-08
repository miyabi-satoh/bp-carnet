// 画面を Playwright で操作する共通の知識。
import { expect, type Locator, type Page, type Response } from '@playwright/test';
import ja from '../messages/ja.json' with { type: 'json' };
// `.ts` まで書くのは、ocr-mocks.ts 経由で scripts/shots.ts が Node で直接読むため。
import { requiredLabel } from '../src/lib/required-label.ts';

/** `ja` は画面の文言。文言の変更に追随するよう messages/ja.json から読む (scripts/shots.ts と同じ)。
 * `requiredLabel` は単体テストと共用 (→ src/lib/required-label.ts)。 */
export { ja, requiredLabel };

/** `method` の API への応答か。`apiPath` は `/api/v1` を除いたパス (`send` と同じ、→ api-helpers.ts)。 */
export function isApiResponse(res: Response, method: string, apiPath: string): boolean {
	return res.request().method() === method && new URL(res.url()).pathname === `/api/v1${apiPath}`;
}

/** 画面の操作が送る API の応答を待つ。操作の前に呼び、返った Promise を操作の後に待つ。 */
export function waitForApiResponse(page: Page, method: string, apiPath: string): Promise<Response> {
	return page.waitForResponse((res) => isApiResponse(res, method, apiPath));
}

/** `button` を押して開くファイルの選択に、`file` を渡す。 */
export async function chooseFile(
	page: Page,
	button: Locator,
	file: { name: string; mimeType: string; buffer: Buffer }
): Promise<void> {
	const chooser = page.waitForEvent('filechooser');
	await button.click();
	await (await chooser).setFiles(file);
}

/**
 * アプリの画面とは別の空のページで `draw(arg)` を動かし、返った data URL (`mimeType`) の中身を返す。
 * 画像の描画・変換は Chromium の canvas で行い、画像処理の依存を足さない。
 */
export async function drawInBlankPage<Arg>(
	page: Page,
	mimeType: string,
	draw: (arg: Arg) => string | Promise<string>,
	arg: Arg
): Promise<Buffer> {
	const blank = await page.context().newPage();
	try {
		// FIX: evaluate の引数の型 (Unboxed<Arg>) は型引数のままでは Arg と照合できないため、引数の型を外して渡す。
		const dataUrl = await blank.evaluate(draw as (arg: unknown) => string | Promise<string>, arg);
		if (!dataUrl.startsWith(`data:${mimeType};`)) throw new Error(`${mimeType} を作れなかった`);
		return Buffer.from(dataUrl.slice(dataUrl.indexOf(',') + 1), 'base64');
	} finally {
		await blank.close();
	}
}

/** 画面を開いたまま、ログアウトした状態にする。先にページを離れる。画面の通信の応答が、消した後に
 * セッションの Cookie を付け直すことがあり、付け直されるとログイン済みのまま (ログイン画面がホーム画面へ送る)。 */
export async function clearLogin(page: Page): Promise<void> {
	await page.goto('about:blank');
	await page.context().clearCookies();
}

/** 要素の位置と大きさ。アニメーションの途中を読まないよう、2回続けて同じになるまで待つ。 */
export async function settledBox(locator: Locator) {
	let previous = '';
	// 代入が poll の中だけなので、初期値の null に型を狭められないよう `as` で宣言の型にする。
	let box = null as Awaited<ReturnType<Locator['boundingBox']>>;
	await expect
		.poll(async () => {
			box = await locator.boundingBox();
			const current = JSON.stringify(box);
			const settled = box !== null && current === previous;
			previous = current;
			return settled;
		})
		.toBe(true);
	if (box === null) throw new Error('要素の位置を取れなかった');
	return box;
}

type PrintCounter = { e2ePrintCalls: number };

/** `window.print` を、呼ばれた回数を数えるだけのものに差し替える。ページを開く前に呼ぶ。 */
export async function countPrints(page: Page) {
	await page.addInitScript(() => {
		const counter = window as unknown as PrintCounter;
		counter.e2ePrintCalls = 0;
		window.print = () => {
			counter.e2ePrintCalls += 1;
		};
	});
	return () => page.evaluate(() => (window as unknown as PrintCounter).e2ePrintCalls);
}

/**
 * 並列実行するworker同士や同一ミリ秒内の複数実行でも衝突しない識別子を作る
 * (Date.now()だけでは2workerが同一ミリ秒に開始した場合に衝突しうるため)。
 */
export function uniqueId(prefix: string): string {
	return `${prefix}-${Date.now()}-${Math.random().toString(36).slice(2, 8)}`;
}

/** 利用規約への同意の文 (`terms_consent_*`) の、差し込み口にリンクの文言を入れた全文。
 * リンクは別のタブで開くので、読み上げ用の知らせ (画面には出ない) も続く。 */
export function consentText(message: string): string {
	return message
		.replace('{terms}', ja.terms_title + ja.common_opens_in_new_tab)
		.replace('{privacy}', ja.privacy_title + ja.common_opens_in_new_tab);
}

/** `upper` の下端が `lower` の上端より上にあるか。両方とも見えていること。 */
export async function isAbove(upper: Locator, lower: Locator): Promise<boolean> {
	const [upperBox, lowerBox] = await Promise.all([upper.boundingBox(), lower.boundingBox()]);
	if (upperBox === null || lowerBox === null) throw new Error('比べる要素が見えていません');
	return upperBox.y + upperBox.height <= lowerBox.y;
}
