// 画面が撮れる・確かめられる状態になるまで待つ。`just spec` (shots.ts) と e2e (frontend/e2e) で共有する。
// `networkidle` はSPAの継続通信 (ポーリング等) で待ち続けて不安定になりがちなため使わず、
// 画面ごとの完了の目印を待つ。

import type { Page, Response } from 'playwright';

/** フォントの読み込み完了。撮影前の共通の前提。 */
export async function waitForFonts(page: Page): Promise<void> {
	await page.evaluate(() => document.fonts.ready);
}

/** LayerChart は ResizeObserver でコンテナのサイズを確定してから各系列の折れ線
 * (`.lc-path`、系列ごとに1本) を描画するため、DOM反映直後ではなく実際に `.lc-path` へ
 * `d` 属性が付くまでポーリングして待つ。
 * 値の無い系列は線が引かれないので、`seriesCount` は値のある系列の数を渡す。 */
export async function waitForChart(page: Page, seriesCount: number, timeout = 5000): Promise<void> {
	await page.waitForFunction(
		(count) => {
			const paths = document.querySelectorAll('.lc-path');
			return paths.length >= count && Array.from(paths).every((p) => p.getAttribute('d'));
		},
		seriesCount,
		{ timeout }
	);
}

/** ログイン画面の `googleEnabled` (Google ログインボタンの出し分け) は `onMount` の
 * 非同期フェッチ (`/api/v1/auth/providers`) 完了後に確定する。ボタンの有無でカードの高さが
 * 変わる前の状態を撮らないよう、ログイン画面を開く前に呼び、返った Promise を撮影前に待つ。 */
export function waitForAuthProviders(page: Page): Promise<Response> {
	return page.waitForResponse((res) => res.url().endsWith('/api/v1/auth/providers'));
}

/** ログイン画面 (`url`) を開き、`/api/v1/auth/providers` の応答まで待つ (→ waitForAuthProviders)。 */
export async function openLoginPage(page: Page, url = '/login'): Promise<void> {
	const providers = waitForAuthProviders(page);
	await page.goto(url);
	await providers;
}
