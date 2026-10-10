// 写真の読み取りを、本物の Gemini で流す (`@ocr`)。利用回数を消費するため、`E2E_OCR=1` のときだけ流す (→ playwright.config.ts)。
// 画面の流れだけなら、Gemini を呼ばない help.e2e.ts・photo.e2e.ts (→ ocr-mocks.ts) で足りる。こちらは実物の写真が読めることを確かめる。
//
// テスト画像は git に入れず、E2E を流すマシンの `data/ocr-test-images/` に置く (実物の写真で、手書きの記録は個人の記録のため)。
// 検証用の app に当てるときも、写真をアップロードするのは流す側のブラウザなので、サーバー側には要らない。
// 読み取り結果は毎回揺れうるので、読めた値そのものは、液晶の数字のようにはっきり写っているものだけで確かめる。
import { readFile } from 'node:fs/promises';
import path from 'node:path';
import type { Page } from '@playwright/test';
import { userTest, expect, NO_LOGIN } from './fixtures';
import { send } from './api-helpers';
import { ja, waitForApiResponse } from './helpers';
import { HomePage } from './pages/home-page';
import { dataDir } from '../scripts/repo-paths.ts';

const IMAGES_DIR = path.join(dataDir(), 'ocr-test-images');

/** Gemini の応答を待つ時間。サーバーが Gemini を待つ上限 (90秒、`src/ocr.rs`) に合わせる。 */
const OCR_TIMEOUT_MS = 90_000;

/** `handwritten-log.jpg` に書かれた記録の行数。一部の行しか読めなかったときに気づけるよう、件数まで確かめる。 */
const HANDWRITTEN_LOG_ROWS = 27;

userTest.describe('写真の読み取り (本物の Gemini)', { tag: '@ocr' }, () => {
	userTest.use({ storageState: NO_LOGIN });
	// 読み取りは1テストに1回だが、使い捨てユーザーの用意・画面の操作・取り込みと後片付けの分も見込む。
	userTest.setTimeout(3 * OCR_TIMEOUT_MS);

	userTest('血圧計の液晶を読み取って記録する', async ({ page }) => {
		const photoPage = await openPhotoPage(page, 'monitor-bp-124-82-60.jpg');
		await expect(photoPage.fields.systolic).toHaveValue('124', { timeout: OCR_TIMEOUT_MS });
		await expect(photoPage.fields.diastolic).toHaveValue('82');
		await expect(photoPage.fields.pulse).toHaveValue('60');

		const created = waitForApiResponse(page, 'POST', '/records');
		await photoPage.createButton.click();
		expect((await created).status()).toBe(201);
		await page.waitForURL('/');
	});

	userTest('血圧値の出ていない液晶は、読み取れなかったと伝える', async ({ page }) => {
		const photoPage = await openPhotoPage(page, 'monitor-datetime-display.jpg');
		await expect(photoPage.noValuesMessage).toBeVisible({
			timeout: OCR_TIMEOUT_MS
		});
	});

	userTest('手書きの記録をまとめて取り込む', async ({ page }) => {
		const photoPage = await openPhotoPage(page, 'handwritten-log.jpg');
		await expect(photoPage.reviewButton).toBeEnabled({ timeout: OCR_TIMEOUT_MS });
		await expect(photoPage.rows.items).toHaveCount(HANDWRITTEN_LOG_ROWS);

		await photoPage.reviewButton.click();
		await expect(photoPage.preview.heading).toBeVisible();
		await expect(page.getByText(ja.import_preview_status_added, { exact: true })).toHaveCount(
			HANDWRITTEN_LOG_ROWS
		);
		const imported = waitForApiResponse(page, 'POST', '/records/import');
		await photoPage.preview.confirmButton.click();
		expect((await imported).ok()).toBe(true);
		await page.waitForURL('/');
	});
});

/** メイン画面の「写真で記録...」で `fileName` のテスト画像を選び、写真で記録のページへ移るのを待つ。 */
async function openPhotoPage(page: Page, fileName: string) {
	const buffer = await readFile(path.join(IMAGES_DIR, fileName)).catch((err: unknown) => {
		throw new Error(`テスト画像を ${IMAGES_DIR} に置く: ${fileName}`, { cause: err });
	});
	const home = new HomePage(page);
	const status = waitForApiResponse(page, 'GET', '/ocr/status');
	await home.open();
	await status;
	// 同意のダイアログは photo.e2e.ts で確かめる。ここでは先に同意しておく。
	await send(page.request, 'post', '/ocr/consent');
	// 読み取りが無効 (サーバーに GEMINI_API_KEY が無い) だとボタンが消える。待ち続けずに理由を出す。
	await expect(
		home.photoButton,
		'サーバーで写真の読み取りが有効になっていない (GEMINI_API_KEY)'
	).toBeVisible();
	return home.pickPhoto({ name: fileName, mimeType: 'image/jpeg', buffer });
}
