// CSV の書き出しと取り込み。取り込みは日付の範囲を丸ごと置き換えるため、必ず使い捨てユーザーで流す。
import { readFile } from 'node:fs/promises';
import type { Page } from '@playwright/test';
import { createRecordByApi, createRecordsByApi, fetchTimeZone } from './api-helpers';
import { todayIn } from './date-helpers';
import { userTest as test, expect, NO_LOGIN } from './fixtures';
import { ja, waitForApiResponse } from './helpers';
import { HomePage } from './pages/home-page';
import { SettingsPage } from './pages/settings-page';

test.use({ storageState: NO_LOGIN });

/** 設定画面の「CSVファイルを読み込む...」で `csv` を選び、取り込みのページへ移るのを待つ。 */
async function openImportPage(page: Page, csv: string | Buffer) {
	const settings = new SettingsPage(page);
	await settings.open();
	const buffer = typeof csv === 'string' ? Buffer.from(csv) : csv;
	return settings.pickCsv({ name: 'records.csv', mimeType: 'text/csv', buffer });
}

test('記録を書き出すと、BOM 付き UTF-8・CRLF 改行で全件が書き出される', async ({ page }) => {
	await createRecordsByApi(page.request, [
		{
			localMeasuredAt: '2026-09-01T21:30',
			systolic: 130,
			diastolic: 85,
			memo: '薬を飲んだ, "少し"頭痛'
		},
		{ localMeasuredAt: '2026-09-01T07:00', systolic: 120, diastolic: 80, pulse: 70, memo: '起床後' }
	]);

	const settings = new SettingsPage(page);
	await settings.open();
	const download = await settings.exportCsv();
	// 書き出すたびに見分けられるよう、設定のタイムゾーンでの今日の日付が付く。
	expect(download.suggestedFilename()).toBe(
		`bp-records-${todayIn(await fetchTimeZone(page.request))}.csv`
	);
	const file = await readFile(await download.path());

	expect([...file.subarray(0, 3)]).toEqual([0xef, 0xbb, 0xbf]);
	// 測定日時の古い順で、設定のタイムゾーンの時刻 (UTC に直さない)。脈拍の無い行は空欄、
	// `,` や `"` を含むメモはクォートされる。
	expect(file.subarray(3).toString('utf8').split('\r\n')).toEqual([
		'measuredAt,systolic,diastolic,pulse,memo',
		'2026-09-01 07:00,120,80,70,起床後',
		'2026-09-01 21:30,130,85,,"薬を飲んだ, ""少し""頭痛"',
		''
	]);
});

// 日本語の Excel で「CSV (コンマ区切り)」を選んで保存し直した形 (docs/import-export.md)。
test('Excel で保存し直した CSV (Shift_JIS・2026/8/20 7:10 の形) を取り込める', async ({ page }) => {
	const csv = Buffer.concat([
		Buffer.from('measuredAt,systolic,diastolic,memo\r\n2026/8/20 7:10,126,84,'),
		// 「起床後」の Shift_JIS。
		Buffer.from([0x8b, 0x4e, 0x8f, 0xb0, 0x8c, 0xe3]),
		Buffer.from('\r\n')
	]);
	const { preview, reviewButton } = await openImportPage(page, csv);
	await reviewButton.click();
	await expect(preview.row('起床後')).toContainText(ja.import_preview_status_added);
	await expect(preview.row('起床後')).toContainText('07:10');
});

// 選んだファイルはページの中だけに持って渡すので、移るときに読み込み直すと消える (docs/import-export.md)。
test('設定画面を開いた後にサーバーへ届かなくなっても、選んだ CSV を取り込みのページへ渡す', async ({
	page
}) => {
	// 開いたときに読み込むコードが届き終わるまで待ち、その後のコードの取得をすべて断る
	// (サーバーが止まった・起動中のときと同じく、移るときにコードを取れない)。届き終わったかは、
	// ページごとのコード (`nodes/`) が4つ (ルートの layout・エラー画面・設定画面・先読みした取り込み) 届いたかで見る。
	const nodes = new Set<string>();
	let inFlight = 0;
	let blocked = false;
	await page.route('**/_app/immutable/**', async (route) => {
		if (blocked) return route.abort();
		inFlight++;
		try {
			const response = await route.fetch();
			if (route.request().url().includes('/_app/immutable/nodes/')) {
				nodes.add(route.request().url());
			}
			await route.fulfill({ response });
		} finally {
			inFlight--;
		}
	});
	const settings = new SettingsPage(page);
	await settings.open();
	await expect.poll(() => nodes.size >= 4 && inFlight === 0).toBe(true);
	blocked = true;

	const { reviewButton } = await settings.pickCsv({
		name: 'records.csv',
		mimeType: 'text/csv',
		buffer: Buffer.from('measuredAt,systolic,diastolic\r\n2026-08-20T07:10,126,84\r\n')
	});
	await expect(reviewButton).toBeVisible();
});

test('取り込みで値の正しくない行を直し、差分を確かめて確定すると設定画面に戻り、一覧に出る', async ({
	page
}) => {
	await createRecordsByApi(page.request, [
		{ localMeasuredAt: '2026-08-09T07:00', systolic: 118, diastolic: 76, memo: '範囲外の記録' },
		{
			localMeasuredAt: '2026-08-10T07:00',
			systolic: 120,
			diastolic: 80,
			pulse: 70,
			memo: '起床後'
		},
		{ localMeasuredAt: '2026-08-10T21:00', systolic: 135, diastolic: 88, memo: '消える記録' }
	]);
	// 置き換える範囲は 8月10日〜11日。10日の朝は既存と同じ、夜は CSV に無いので消える。
	const csv = [
		'measuredAt,systolic,diastolic,pulse,memo',
		'2026-08-10T07:00,120,80,70,起床後',
		'2026-08-11T07:00,abc,78,,直す行',
		'2026-08-11T21:00,300,90,,直さない行',
		'2026-08-11T21:30,126,84,66,追加する記録'
	].join('\r\n');

	const { rows, preview, reviewButton: review } = await openImportPage(page, csv);

	await expect(rows.heading).toBeVisible();
	// 読み込んだ4行すべてを並べ、そのままでは取り込めない2行はチェックを外しておく。
	await expect(rows.items).toHaveCount(4);
	await expect(rows.count(4, 2)).toBeVisible();

	// 数字でない値も、記録フォームと同じ文言で行に出す。直すのは行から開くシート。
	const notInteger = ja.record_error_not_integer.replace('{field}', ja.record_systolic_label);
	const fixRow = rows.row('直す行');
	await expect(fixRow).toContainText(notInteger);
	await rows.fixSystolic(fixRow, '118');
	// 直すとエラーが消え、チェックも入る。
	await expect(fixRow).not.toContainText(notInteger);
	await expect(rows.count(4, 3)).toBeVisible();
	await expect(rows.row('直さない行')).toContainText(
		ja.record_error_systolic_range.replace('{min}', '60').replace('{max}', '260')
	);

	await review.click();
	await expect(preview.heading).toBeVisible();
	await expect(preview.row('起床後')).toContainText(ja.import_preview_status_unchanged);
	await expect(preview.row('消える記録')).toContainText(ja.import_preview_status_removed);
	await expect(preview.row('直す行')).toContainText(ja.import_preview_status_added);
	await expect(preview.row('追加する記録')).toContainText(ja.import_preview_status_added);
	await expect(preview.row('直さない行')).toHaveCount(0);
	await expect(preview.row('範囲外の記録')).toHaveCount(0);
	await expect(preview.deletionWarning).toBeVisible();

	// 端末の戻る操作で一覧へ戻っても、直した値は残り、離れる確認も出ない。
	await page.goBack();
	await expect(fixRow).toContainText(/118\s*\/\s*78/);
	await expect(page.getByRole('alertdialog')).toHaveCount(0);

	// ページの中の段階は、読み込んだ内容がページに残っているので「進む」で戻せる
	// (中身を戻せないシート・拡大表示とは扱いが違う)。
	await page.goForward();
	await expect(preview.heading).toBeVisible();
	await page.goBack();
	await expect(fixRow).toContainText(/118\s*\/\s*78/);

	await review.click();

	const imported = waitForApiResponse(page, 'POST', '/records/import');
	await preview.confirmButton.click();
	expect((await imported).ok()).toBe(true);
	// 確定したら、取り込みのページを履歴に残さずに設定画面へ戻る。
	await page.waitForURL('/settings');

	const home = new HomePage(page);
	await home.open();
	await home.periodNav.filter('2026-08-09', '2026-08-11');
	await expect(home.recordCard('範囲外の記録').root).toBeVisible();
	// 同じ記録が二重に取り込まれていないことを、画面の文言の数で確かめる。
	await expect(page.getByText('起床後', { exact: true })).toHaveCount(1);
	await expect(home.recordCard('直す行').root).toContainText(/118\s*\/\s*78/);
	await expect(home.recordCard('追加する記録').root).toBeVisible();
	await expect(page.getByText('消える記録', { exact: true })).toHaveCount(0);
	await expect(page.getByText('直さない行', { exact: true })).toHaveCount(0);
});

test('取り込みで値を直した後にページを離れようとすると確かめ、とどまれば直した値が残る', async ({
	page
}) => {
	const csv = ['measuredAt,systolic,diastolic', '2026-08-11T07:00,abc,78'].join('\r\n');

	const importPage = await openImportPage(page, csv);
	const { rows, leaveConfirm } = importPage;
	await expect(rows.noneKeptDescription).toBeVisible();
	const row = rows.items.first();
	await rows.fixSystolic(row, '118');

	await importPage.backButton.click();
	await expect(leaveConfirm.root).toBeVisible();
	await leaveConfirm.stayButton.click();
	await expect(leaveConfirm.root).toBeHidden();
	await expect(page).toHaveURL('/settings/import');
	await expect(row).toContainText(/118\s*\/\s*78/);

	// 端末の戻る操作でも確かめる。
	await page.goBack();
	await expect(leaveConfirm.root).toBeVisible();
	await leaveConfirm.leaveButton.click();
	await page.waitForURL('/settings');
});

// 確認画面を出してから確定するまでの間に置き換える日の記録がほかで変わったら、取り込まずに
// 確認画面を最新の記録で出し直す (docs/import-export.md)。ほかの端末の操作は同じログイン状態の API で行う。
test('確認画面を出した後にほかで記録が足されたら、取り込まずに出し直し、確かめてから取り込める', async ({
	page
}) => {
	await createRecordByApi(page.request, {
		localMeasuredAt: '2026-08-20T07:00',
		systolic: 118,
		diastolic: 76,
		memo: '元の記録'
	});
	const csv = ['measuredAt,systolic,diastolic,memo', '2026-08-20T21:00,126,84,取り込む記録'].join(
		'\r\n'
	);
	const { preview, reviewButton } = await openImportPage(page, csv);
	await reviewButton.click();
	await expect(preview.row('元の記録')).toContainText(ja.import_preview_status_removed);

	await createRecordByApi(page.request, {
		localMeasuredAt: '2026-08-20T12:00',
		systolic: 130,
		diastolic: 85,
		memo: 'ほかで足した記録'
	});
	const confirm = preview.confirmButton;
	const conflicted = waitForApiResponse(page, 'POST', '/records/import');
	await confirm.click();
	expect((await conflicted).status()).toBe(409);

	await expect(preview.changedElsewhereNotice).toBeVisible();
	await expect(preview.row('ほかで足した記録')).toContainText(ja.import_preview_status_removed);
	await expect(page).toHaveURL('/settings/import');

	const imported = waitForApiResponse(page, 'POST', '/records/import');
	await confirm.click();
	expect((await imported).ok()).toBe(true);
	await page.waitForURL('/settings');
	// 設定画面には記録が出ないので、取り込めたことをお知らせで伝える。
	await expect(new SettingsPage(page).importedNotice).toContainText(
		ja.settings_import_done_description.replace('{count}', '1')
	);

	const home = new HomePage(page);
	await home.open();
	await home.periodNav.filter('2026-08-20', '2026-08-20');
	await expect(home.recordCard('取り込む記録').root).toBeVisible();
	await expect(page.getByText('ほかで足した記録', { exact: true })).toHaveCount(0);
});

test('取り込みの送信中は、端末の戻る操作でもページを離れず、終わってから設定画面へ戻る', async ({
	page
}) => {
	const csv = ['measuredAt,systolic,diastolic', '2026-08-11T07:00,120,78'].join('\r\n');
	let release: () => void = () => {};
	const released = new Promise<void>((resolve) => (release = resolve));
	await page.route('**/api/v1/records/import', async (route) => {
		await released;
		await route.continue();
	});

	const { preview, reviewButton } = await openImportPage(page, csv);
	// 直すところが無くても、取り込む行を確かめる一覧は飛ばさない (docs/import-export.md)。
	await reviewButton.click();
	const confirm = preview.confirmButton;
	await expect(confirm).toBeEnabled();
	const imported = waitForApiResponse(page, 'POST', '/records/import');
	await confirm.click();

	await page.goBack();
	await expect(page).toHaveURL('/settings/import');
	await expect(page.getByRole('alertdialog')).toHaveCount(0);

	release();
	expect((await imported).ok()).toBe(true);
	await page.waitForURL('/settings');
});

test('取り込みで時刻の無い行は朝・夜と出し、シートを開くと仮の時刻を振ったと伝える', async ({
	page
}) => {
	const csv = [
		'measuredAt,systolic,diastolic,memo',
		'2026-08-12,120,80,一件目',
		'2026-08-12,130,85,二件目'
	].join('\r\n');

	const { rows } = await openImportPage(page, csv);
	// 時刻の代わりに「朝」「夜」だけを出し、行には断り書きを出さない (docs/ocr.md)。
	const morningRow = rows.row('一件目');
	await expect(morningRow).toContainText(ja.record_day_period_morning_label);
	await expect(morningRow).not.toContainText(/\d{1,2}:\d{2}/);
	await expect(rows.row('二件目')).toContainText(ja.record_day_period_evening_label);
	await expect(page.getByText(ja.import_rows_placeholder_time_notice)).toHaveCount(0);
	await expect(rows.keepCheckbox()).toHaveCount(2);
	for (const keep of await rows.keepCheckbox().all()) {
		await expect(keep).toBeChecked();
	}

	const sheet = await rows.openRow(morningRow);
	await expect(sheet.root.getByText(ja.import_rows_placeholder_time_notice)).toBeVisible();
	await expect(sheet.fields.time).not.toHaveValue('');
});
