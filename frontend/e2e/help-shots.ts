// 使い方のページ (`/help`) の手順の画像。`E2E_HELP_SHOTS=1` (`just help-shots`) のときだけ撮り、
// アプリに同梱する frontend/static/help/ へ書き出す。ファイル名は src/lib/help.ts の helpStepImagePath と揃える。
import { writeFile } from 'node:fs/promises';
import path from 'node:path';
import { expect, type Locator, type Page } from '@playwright/test';
import { drawInBlankPage, settledBox } from './helpers';
import { waitForFonts } from './page-waits';

export const HELP_SHOTS_ENABLED = process.env.E2E_HELP_SHOTS === '1';

const HELP_IMAGES_DIR = path.join(import.meta.dirname, '..', 'static', 'help');

/**
 * ADR: 画像は幅 720px の WebP にする。スマホの画面をそのまま撮った PNG は1枚あたり数百 KB あり、
 * 項目が増えるほど実行ファイルが膨らむ。項目ページでは幅 256px 前後で出すため、画面の密度 3 倍でも
 * 足りる幅に縮める。縮小と WebP への変換は Chromium の canvas で行い、画像処理の依存を足さない。
 * 720 は src/lib/help.ts の HELP_STEP_IMAGE_SIZE と揃える。
 */
const IMAGE_WIDTH = 720;
const WEBP_QUALITY = 0.8;

/** 見本の値。使い捨てユーザーの ID など、テスト用と分かる値の代わりに写す。本番のアカウントに合わせてメールアドレスにする。 */
export const SAMPLE_TEXT = 'sample@example.com';

/** 押す場所の印の色。画面のどの配色とも紛れない赤にする。 */
const HIGHLIGHT_COLOR = '#e5484d';

/**
 * 見えている範囲を撮り、`name` (`<slug>-<手順の番号>`) で書き出す。`highlight` の要素は赤い枠で囲む。
 * `sample` の要素は、文字を見本の値 (`SAMPLE_TEXT`、ログインの手順と同じ) に替えて撮る
 * (使い捨てユーザーの ID など、テスト用と分かる値を写さないため)。
 * `E2E_HELP_SHOTS` が無ければ何もしない。画面ごとの読み込み完了は呼び出し側で待っておく。
 */
export async function takeHelpShot(
	page: Page,
	name: string,
	highlight: Locator[] = [],
	sample: Locator[] = []
): Promise<void> {
	if (!HELP_SHOTS_ENABLED) return;
	await expect(page.locator('html')).not.toHaveClass(/\bdark\b/);
	await waitForFonts(page);
	// ダイアログが開く途中などで測ると、枠がずれる。
	const boxes = await Promise.all(highlight.map((locator) => settledBox(locator)));
	await page.evaluate(
		({ boxes, color }) => {
			for (const box of boxes) {
				const mark = document.createElement('div');
				mark.dataset.helpHighlight = '';
				const pad = 4;
				Object.assign(mark.style, {
					position: 'fixed',
					left: `${box.x - pad}px`,
					top: `${box.y - pad}px`,
					width: `${box.width + pad * 2}px`,
					height: `${box.height + pad * 2}px`,
					border: `3px solid ${color}`,
					borderRadius: '12px',
					pointerEvents: 'none',
					zIndex: '2147483647'
				});
				document.body.append(mark);
			}
		},
		{ boxes, color: HIGHLIGHT_COLOR }
	);
	// 文字を替えると元の文字で探す locator では見つからなくなるため、戻すときのために要素そのものを控える。
	const sampleElements = await Promise.all(sample.map((locator) => locator.elementHandle()));
	const originalTexts = await Promise.all(
		sampleElements.map((element) =>
			element.evaluate((el, sample) => {
				const text = el.textContent ?? '';
				el.textContent = sample;
				return text;
			}, SAMPLE_TEXT)
		)
	);
	try {
		const png = await page.screenshot({ animations: 'disabled' });
		await writeFile(path.join(HELP_IMAGES_DIR, `${name}.webp`), await toWebp(page, png));
	} finally {
		await page.evaluate(() => {
			for (const mark of document.querySelectorAll('[data-help-highlight]')) mark.remove();
		});
		for (const [i, element] of sampleElements.entries()) {
			await element.evaluate((el, text) => (el.textContent = text), originalTexts[i]);
			await element.dispose();
		}
	}
}

/** PNG を幅 `IMAGE_WIDTH` に縮めた WebP にする。 */
async function toWebp(page: Page, png: Buffer): Promise<Buffer> {
	return drawInBlankPage(
		page,
		'image/webp',
		async ({ base64, width, quality }) => {
			const image = new Image();
			image.src = `data:image/png;base64,${base64}`;
			await image.decode();
			const canvas = document.createElement('canvas');
			canvas.width = width;
			canvas.height = Math.round((image.naturalHeight * width) / image.naturalWidth);
			const context = canvas.getContext('2d');
			if (!context) throw new Error('canvas の 2D コンテキストを取れなかった');
			context.imageSmoothingQuality = 'high';
			context.drawImage(image, 0, 0, canvas.width, canvas.height);
			return canvas.toDataURL('image/webp', quality);
		},
		{ base64: png.toString('base64'), width: IMAGE_WIDTH, quality: WEBP_QUALITY }
	);
}
