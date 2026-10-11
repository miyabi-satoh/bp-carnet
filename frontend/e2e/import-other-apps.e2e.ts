// 他のアプリが書き出したファイルの取り込み (docs/import-export.md「他のアプリのファイル」)。
// Gemini は呼ばず、列の意味と PDF の読み取りの応答を差し替える。取り込みは日付を丸ごと置き換えるので、使い捨てユーザーで流す。
import type { Page, Request } from '@playwright/test';
import { exportXml, record, zip } from '../src/lib/apple-health.test-data';
import { userTest as test, expect, NO_LOGIN } from './fixtures';
import { ja, waitForApiResponse } from './helpers';
import {
	errorResponse,
	freeQuota,
	mockDocumentRead,
	mockOcrStatus,
	mockTableColumns,
	tableColumns
} from './ocr-mocks';
import { SettingsPage } from './pages/settings-page';

test.use({ storageState: NO_LOGIN });

/** OMRON connect が書き出す形の CSV。`rows` 件の記録を、2025年1月1日から1日1件並べる。 */
function omronCsv(rows: number): string {
	const lines = Array.from({ length: rows }, (_, i) => {
		const date = new Date(Date.UTC(2025, 0, 1 + i)).toISOString().slice(0, 10);
		return `${date} 7:08,Asia/Tokyo,${120 + (i % 20)},80,65,HEM-7346T`;
	});
	return [
		'測定日,タイムゾーン,最高血圧（mmHg）,最低血圧（mmHg）,脈拍（bpm）,機種',
		...lines,
		''
	].join('\n');
}

const OMRON_COLUMNS = tableColumns({ date: 0, systolic: 2, diastolic: 3, pulse: 4 });

async function pickFile(page: Page, file: { name: string; mimeType: string; buffer: Buffer }) {
	const settings = new SettingsPage(page);
	await settings.open();
	return settings.pickImportFile(file);
}

/** Gemini への読み取りの API に届いたリクエストを集める。 */
function collectReadRequests(page: Page): Request[] {
	const requests: Request[] = [];
	page.on('request', (request) => {
		if (/\/api\/v1\/ocr\/(table-columns|document)$/.test(request.url())) requests.push(request);
	});
	return requests;
}

test('他のアプリの CSV は、先頭の数行だけを送ることに同意してから読み込み、取り込める', async ({
	page
}) => {
	await mockOcrStatus(page);
	await mockTableColumns(page, { json: OMRON_COLUMNS });
	const importPage = await pickFile(page, {
		name: 'omron.csv',
		mimeType: 'text/csv',
		buffer: Buffer.from(omronCsv(30))
	});

	// 同意するまで送らない。送るもの (先頭の行数) をダイアログに出す。
	const consent = importPage.fileReadConsent;
	await expect(consent.root).toBeVisible();
	await expect(
		consent.root.getByText(ja.file_read_consent_table_item_data.replace('{lines}', '12'))
	).toBeVisible();

	const sent = page.waitForRequest('**/api/v1/ocr/table-columns');
	await consent.agreeButton.click();
	// 送るのは見出しと先頭の11件だけで、残りの記録は送らない。
	const sample: string = (await sent).postDataJSON().sample;
	expect(sample.split('\n')).toHaveLength(12);
	expect(sample).toContain('2025-01-11 7:08');
	expect(sample).not.toContain('2025-01-12 7:08');

	// 送らなかった行も含めて、全部の行を端末の中で記録にする。
	await expect(importPage.rows.count(30, 30)).toBeVisible();
	await importPage.reviewButton.click();
	await expect(page.getByText(ja.import_preview_status_added, { exact: true })).toHaveCount(30);

	const imported = waitForApiResponse(page, 'POST', '/records/import');
	await importPage.preview.confirmButton.click();
	expect((await (await imported).json()).created).toHaveLength(30);
	await page.waitForURL('/settings');
});

test('送ることに同意しなければ、何も送らずにファイルを選ぶところへ戻る', async ({ page }) => {
	await mockOcrStatus(page);
	await mockTableColumns(page, { json: OMRON_COLUMNS });
	const requests = collectReadRequests(page);
	const importPage = await pickFile(page, {
		name: 'omron.csv',
		mimeType: 'text/csv',
		buffer: Buffer.from(omronCsv(3))
	});

	await importPage.fileReadConsent.declineButton.click();
	await expect(importPage.pickFileButton).toBeVisible();
	expect(requests).toHaveLength(0);
});

test('血圧の記録の表でないファイルは、読み取れなかったと伝える', async ({ page }) => {
	await mockOcrStatus(page);
	await mockTableColumns(page, { json: { ...OMRON_COLUMNS, readable: false, groups: [] } });
	const importPage = await pickFile(page, {
		name: 'items.csv',
		mimeType: 'text/csv',
		buffer: Buffer.from('Item,Price\nCuff,49.99\n')
	});

	await importPage.fileReadConsent.agreeButton.click();
	await expect(importPage.error(ja.import_error_table_unreadable)).toBeVisible();
	await expect(importPage.pickFileButton).toBeVisible();
});

test('テキストでないファイル (Excel のブックなど) は、送らずに選び直すよう伝える', async ({
	page
}) => {
	await mockOcrStatus(page);
	const requests = collectReadRequests(page);
	const importPage = await pickFile(page, {
		name: 'book.xlsx',
		mimeType: 'application/vnd.openxmlformats-officedocument.spreadsheetml.sheet',
		buffer: Buffer.from([0x50, 0x4b, 0x03, 0x04, 0x14, 0x00, 0x06, 0x00, 0x08, 0x00, 0x00, 0x00])
	});

	await expect(importPage.error(ja.import_error_not_text)).toBeVisible();
	await expect(importPage.fileReadConsent.root).toBeHidden();
	expect(requests).toHaveLength(0);
});

test('大きすぎる表の形のファイルは、読み込まずに選び直すよう伝える', async ({ page }) => {
	await mockOcrStatus(page);
	const requests = collectReadRequests(page);
	const importPage = await pickFile(page, {
		name: 'huge.csv',
		mimeType: 'text/csv',
		// 上限 (`TABLE_MAX_BYTES`、20MB) を1バイト超える大きさ。
		buffer: Buffer.alloc(20 * 1024 * 1024 + 1, 'a')
	});

	await expect(importPage.error(ja.import_error_file_too_large)).toBeVisible();
	await expect(importPage.fileReadConsent.root).toBeHidden();
	expect(requests).toHaveLength(0);
});

test('読み取りが使えないときは、このアプリの CSV でないことを伝え、同意は求めない', async ({
	page
}) => {
	await mockOcrStatus(page, { enabled: false });
	const importPage = await pickFile(page, {
		name: 'omron.csv',
		mimeType: 'text/csv',
		buffer: Buffer.from(omronCsv(3))
	});

	await expect(page.getByText(/このアプリで書き出したCSVファイルではないようです/)).toBeVisible();
	await expect(importPage.fileReadConsent.root).toBeHidden();
});

test('読み取りの枠を使い切っていれば、同意を求める前に伝える', async ({ page }) => {
	await mockOcrStatus(page, { quotas: [freeQuota(0)], quotaLow: true });
	const requests = collectReadRequests(page);
	const importPage = await pickFile(page, {
		name: 'report.pdf',
		mimeType: 'application/pdf',
		buffer: Buffer.from('%PDF-1.7 sample')
	});

	await expect(importPage.error(ja.error_ocr_budget_exhausted)).toBeVisible();
	await expect(importPage.fileReadConsent.root).toBeHidden();
	expect(requests).toHaveLength(0);
});

test('PDF は、ファイルの全体を送ることに同意してから読み取り、自信のない行を要確認にする', async ({
	page
}) => {
	await mockOcrStatus(page);
	await mockDocumentRead(page, {
		json: {
			readings: [
				{
					measuredOn: '2025-12-30',
					time: '07:05',
					systolic: 132,
					diastolic: 86,
					pulse: 64,
					confidence: 0.99,
					memo: '年末'
				},
				// 時刻の無い行は、書かれた朝・夜の時間帯に仮の時刻を振る。
				{
					measuredOn: '2026-01-01',
					time: '',
					systolic: 141,
					diastolic: 92,
					pulse: 0,
					confidence: 0.95,
					period: 'evening'
				},
				{
					measuredOn: '2026-01-02',
					time: '08:00',
					systolic: 129,
					diastolic: 84,
					pulse: 62,
					confidence: 0.5
				}
			]
		}
	});
	const importPage = await pickFile(page, {
		name: 'report.pdf',
		mimeType: 'application/pdf',
		buffer: Buffer.from('%PDF-1.7 sample')
	});

	const consent = importPage.fileReadConsent;
	await expect(consent.root.getByText(ja.file_read_consent_document_item_data)).toBeVisible();
	await consent.agreeButton.click();

	// 自信のない行は、チェックを外して「要確認」にする (写真の手帳の読み取りと同じ)。
	await expect(importPage.rows.count(3, 2)).toBeVisible();
	await expect(importPage.rows.row('129')).toContainText(ja.import_rows_needs_review_badge);
	await expect(importPage.rows.row('141')).toContainText(ja.record_day_period_evening_label);
	await expect(importPage.rows.row('132')).toContainText('2025年');
});

test('PDF の記録が多すぎて読み取れなければ、期間を分けるよう伝える', async ({ page }) => {
	await mockOcrStatus(page);
	await mockDocumentRead(page, errorResponse(422, 'ocr_document_too_long'));
	const importPage = await pickFile(page, {
		name: 'report.pdf',
		mimeType: 'application/pdf',
		buffer: Buffer.from('%PDF-1.7 sample')
	});

	await importPage.fileReadConsent.agreeButton.click();
	await expect(importPage.error(ja.error_ocr_document_too_long)).toBeVisible();
});

test('「ヘルスケア」の書き出し (zip) は、どこにも送らずに血圧と脈拍を読み込む', async ({
	page
}) => {
	const requests = collectReadRequests(page);
	const morning = '2025-02-05 07:08:33 +0900';
	const evening = '2025-02-05 21:40:02 +0900';
	const xml = exportXml(
		record('HeartRate', morning, '89', 'count/min') +
			record('BloodPressureSystolic', morning, '140') +
			record('BloodPressureDiastolic', morning, '106') +
			record('BloodPressureSystolic', evening, '132') +
			record('BloodPressureDiastolic', evening, '98')
	);
	const importPage = await pickFile(page, {
		name: '書き出したデータ.zip',
		mimeType: 'application/zip',
		buffer: Buffer.from(
			zip([
				{ name: 'apple_health_export/export_cda.xml', content: '<ClinicalDocument/>' },
				{ name: 'apple_health_export/書き出したデータ.xml', content: xml }
			])
		)
	});

	await expect(importPage.rows.count(2, 2)).toBeVisible();
	await expect(importPage.rows.row('140')).toContainText('89');
	await expect(importPage.rows.row('132')).toContainText('21:40');
	await expect(importPage.fileReadConsent.root).toBeHidden();
	expect(requests).toHaveLength(0);
});

test('「ヘルスケア」の書き出しでない zip は、選び直すよう伝える', async ({ page }) => {
	const importPage = await pickFile(page, {
		name: 'photos.zip',
		mimeType: 'application/zip',
		buffer: Buffer.from(zip([{ name: 'photos/a.txt', content: 'hello' }]))
	});

	await expect(importPage.error(ja.import_error_apple_health_not_export)).toBeVisible();
});
