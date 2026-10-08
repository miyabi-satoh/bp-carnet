// 写真で記録のページ (`/record/photo`) の段階の行き来と、離れる前の確認。読み取りはモックにする (→ ocr-mocks.ts)。
// 取り込みは日付の範囲を丸ごと置き換えるため、使い捨てユーザーで流す。
import type { Locator, Page } from '@playwright/test';
import { createRecordByApi, fetchTimeZone } from './api-helpers';
import { addDays, todayIn, weekLabel } from './date-helpers';
import { userTest as test, expect, NO_LOGIN } from './fixtures';
import { chooseFile, ja, settledBox, waitForApiResponse } from './helpers';
import { HomePage } from './pages/home-page';
import { PhotoPage } from './pages/photo-page';
import type { PhotoPreview } from './pages/photo-preview';
import {
	errorResponse,
	freeQuota,
	mockOcrStatus,
	mockOcrExtract,
	monitorResult,
	noValuesResult,
	notebookResult,
	paidQuota,
	samplePhoto,
	type NotebookRow
} from './ocr-mocks';

test.use({ storageState: NO_LOGIN });

/** 読み取りが `result` を返すようにし、ホーム画面の「写真で記録...」で `photo` の写真 (`exif` を渡せばその Exif 付き) を選んでページへ移る。 */
async function openPhotoPageFromHome(
	page: Page,
	result: unknown,
	photo: Parameters<typeof samplePhoto>[1],
	exif?: Parameters<typeof samplePhoto>[2]
): Promise<PhotoPage> {
	await mockOcrStatus(page);
	await mockOcrExtract(page, { json: result });
	const home = new HomePage(page);
	await home.open();
	return home.pickPhoto(await samplePhoto(page, photo, exif));
}

/** 手帳の読み取り結果を返すようにし、ホーム画面の「写真で記録...」で写真を選んでページへ移る。 */
async function openNotebookRows(page: Page): Promise<PhotoPage> {
	// いつ流しても先の日付にならないよう、昨日の朝・夜にする。
	const yesterday = addDays(todayIn(await fetchTimeZone(page.request)), -1);
	const rows: NotebookRow[] = [
		{ date: yesterday, time: '07:00', systolic: 120, diastolic: 80, pulse: 70 },
		{ date: yesterday, time: '21:00', systolic: 124, diastolic: 82, pulse: 68 }
	];
	const photoPage = await openPhotoPageFromHome(page, notebookResult(rows), {
		kind: 'notebook',
		rows
	});
	await expect(photoPage.rows.items).toHaveCount(rows.length);
	return photoPage;
}

// 手帳に「夜」とだけ書いた日の1件を、並び順の規則で朝にしない (docs/ocr.md)。
test('手帳の時刻の無い行は、行に書かれた朝・夜の時間帯に振る', async ({ page }) => {
	const yesterday = addDays(todayIn(await fetchTimeZone(page.request)), -1);
	const rows: NotebookRow[] = [
		{ date: yesterday, systolic: 131, diastolic: 85, pulse: 64, memo: '夜だけ', period: 'evening' },
		{ date: addDays(yesterday, -1), systolic: 126, diastolic: 80, pulse: 68, memo: '書き込み無し' }
	];
	const { rows: importRows } = await openPhotoPageFromHome(page, notebookResult(rows), {
		kind: 'notebook',
		rows
	});
	// CSV の取り込みと同じ見出しと説明を出す。
	await expect(importRows.heading).toBeVisible();
	await expect(importRows.description).toBeVisible();
	await expect(importRows.row('夜だけ')).toContainText(ja.record_day_period_evening_label);
	await expect(importRows.row('書き込み無し')).toContainText(ja.record_day_period_morning_label);
});

// エラーの記録はチェックが残っていても取り込まないので、押せない理由を一覧の上で伝える (docs/import-export.md)。
test('読み取った記録がすべてエラーなら、取り込める記録が無いと伝え、直すと消える', async ({
	page
}) => {
	const yesterday = addDays(todayIn(await fetchTimeZone(page.request)), -1);
	const rows: NotebookRow[] = [
		{ date: yesterday, time: '07:00', systolic: 72, diastolic: 84, pulse: 70 }
	];
	const { rows: importRows, noneKeptNote } = await openPhotoPageFromHome(
		page,
		notebookResult(rows),
		{ kind: 'notebook', rows }
	);
	await expect(noneKeptNote).toBeVisible();
	await expect(importRows.keepCheckbox()).toBeDisabled();

	// 直すシートは、開いた時点からどこを直せばよいかを出す (docs/import-export.md)。
	const sheet = await importRows.openRow(importRows.items.first());
	await expect(
		sheet.root.getByText(ja.record_error_systolic_not_greater, { exact: true })
	).toBeVisible();
	await sheet.fields.systolic.fill('120');
	await sheet.updateButton.click();
	await expect(sheet.root).toBeHidden();
	await expect(noneKeptNote).toBeHidden();
	await expect(importRows.keepCheckbox()).toBeChecked();
});

test('読み取りに自信の無い記録は「要確認」を付けて薄くせず、直すと「修正済み」にして絞り込みにも残す', async ({
	page
}) => {
	const yesterday = addDays(todayIn(await fetchTimeZone(page.request)), -1);
	const rows: NotebookRow[] = [
		{ date: yesterday, time: '07:00', systolic: 120, diastolic: 80, pulse: 70 },
		{ date: yesterday, time: '21:00', systolic: 138, diastolic: 88, pulse: 66 }
	];
	const result = notebookResult(rows);
	result.readings![1].confidence = 0.5;
	const { rows: importRows } = await openPhotoPageFromHome(page, result, {
		kind: 'notebook',
		rows
	});

	const unsure = importRows.row('21:00');
	await expect(unsure.getByText(ja.import_rows_needs_review_badge, { exact: true })).toBeVisible();
	await expect(importRows.keepCheckbox(unsure)).not.toBeChecked();
	// チェックが外れていても、確かめてほしい記録は薄くしない (docs/import-export.md)。
	await expect(unsure).toHaveCSS('opacity', '1');

	await importRows.needsReviewFilter.click();
	await expect(importRows.items).toHaveCount(1);
	await importRows.fixSystolic(unsure, '136');
	await expect(importRows.items).toHaveCount(1);
	await expect(unsure.getByText(ja.import_rows_fixed_badge, { exact: true })).toBeVisible();
	await expect(unsure.getByText(ja.import_rows_needs_review_badge, { exact: true })).toHaveCount(0);
	await expect(importRows.keepCheckbox(unsure)).toBeChecked();
});

test('読み取った行のチェックを外した後にページを離れようとすると確かめ、とどまればチェックが外れたまま残る', async ({
	page
}) => {
	const photoPage = await openNotebookRows(page);
	const firstKeep = photoPage.rows.keepCheckbox().first();
	await firstKeep.click();
	await expect(firstKeep).not.toBeChecked();

	const { leaveConfirm } = photoPage;
	await photoPage.backButton.click();
	await expect(leaveConfirm.root).toBeVisible();
	await leaveConfirm.stayButton.click();
	await expect(leaveConfirm.root).toBeHidden();
	await expect(page).toHaveURL('/record/photo');
	await expect(firstKeep).not.toBeChecked();

	// 端末の戻る操作でも確かめる。
	await page.goBack();
	await expect(leaveConfirm.root).toBeVisible();
	await leaveConfirm.leaveButton.click();
	await page.waitForURL('/');
});

test('手帳の年を選び直すと、すべての行の年が変わり、その年の記録からメモを引き継ぎ直す', async ({
	page
}) => {
	const today = todayIn(await fetchTimeZone(page.request));
	// 2年前にうるう日が無いと日付がずれるので、2月29日は避ける。
	const date = addDays(today, today.endsWith('-03-01') ? -2 : -1);
	const year = Number(date.slice(0, 4));
	const oldDate = `${year - 2}${date.slice(4)}`;
	await createRecordByApi(page.request, {
		localMeasuredAt: `${oldDate}T07:00`,
		systolic: 120,
		diastolic: 80,
		pulse: 70,
		memo: '2年前のメモ'
	});
	const rows: NotebookRow[] = [{ date, time: '07:00', systolic: 120, diastolic: 80, pulse: 70 }];
	const photoPage = await openPhotoPageFromHome(page, notebookResult(rows), {
		kind: 'notebook',
		rows
	});
	await expect(photoPage.memoYear).toHaveValue(String(year));

	await photoPage.memoYear.selectOption(String(year - 2));

	const row = photoPage.rows.items.first();
	await expect(row).toContainText('2年前のメモ');
	await expect(photoPage.reviewButton).toBeEnabled();
	await photoPage.reviewButton.click();
	// 置き換えるのは選び直した年の日で、2年前の記録と同じなので「変更なし」になる。
	await expect(photoPage.preview.heading).toBeVisible();
	await expect(photoPage.preview.row(ja.import_preview_status_unchanged)).toHaveCount(1);
	await expect(photoPage.preview.deletionWarning).toHaveCount(0);
});

test('年を選び直しても、画面で直したメモは既存の記録のメモで上書きしない', async ({ page }) => {
	const today = todayIn(await fetchTimeZone(page.request));
	const date = addDays(today, today.endsWith('-03-01') ? -2 : -1);
	const year = Number(date.slice(0, 4));
	await createRecordByApi(page.request, {
		localMeasuredAt: `${year - 2}${date.slice(4)}T07:00`,
		systolic: 120,
		diastolic: 80,
		pulse: 70,
		memo: '2年前のメモ'
	});
	const rows: NotebookRow[] = [{ date, time: '07:00', systolic: 120, diastolic: 80, pulse: 70 }];
	const photoPage = await openPhotoPageFromHome(page, notebookResult(rows), {
		kind: 'notebook',
		rows
	});
	const row = photoPage.rows.items.first();
	const sheet = await photoPage.rows.openRow(row);
	await sheet.fields.memo.fill('画面で書いたメモ');
	await sheet.updateButton.click();
	await expect(sheet.root).toBeHidden();

	await photoPage.memoYear.selectOption(String(year - 2));

	await expect(photoPage.reviewButton).toBeEnabled();
	await expect(row).toContainText('画面で書いたメモ');
	await expect(row).not.toContainText('2年前のメモ');
});

test('日付を読み取れなかった行は、今日にせず「要確認」にして、日付を入れるよう添える', async ({
	page
}) => {
	const yesterday = addDays(todayIn(await fetchTimeZone(page.request)), -1);
	const rows: NotebookRow[] = [
		{ date: yesterday, time: '07:00', systolic: 120, diastolic: 80, pulse: 70 }
	];
	const result = notebookResult(rows);
	result.readings![0].measuredOn = null;
	const { rows: importRows, memoYear } = await openPhotoPageFromHome(page, result, {
		kind: 'notebook',
		rows
	});

	const row = importRows.items.first();
	await expect(row).toContainText(ja.import_rows_note_date_unread);
	await expect(importRows.keepCheckbox(row)).toBeDisabled();
	// 日付の入った行が無ければ、年の欄は出さない。
	await expect(memoYear).toHaveCount(0);
});

test('取り込み内容の確認から戻る操作で行の編集に戻っても直した値が残り、確定するとホーム画面へ戻る', async ({
	page
}) => {
	const photoPage = await openNotebookRows(page);
	const firstRow = photoPage.rows.items.first();
	await photoPage.rows.fixSystolic(firstRow, '118');
	const review = photoPage.reviewButton;
	await expect(review).toBeEnabled();
	await review.click();
	await expect(photoPage.preview.heading).toBeVisible();

	// 段階の行き来は、ページを離れないので確かめない。
	await page.goBack();
	await expect(photoPage.heading).toBeVisible();
	await expect(page.getByRole('alertdialog')).toHaveCount(0);
	await expect(firstRow).toContainText(/118\s*\//);

	await review.click();
	const confirm = photoPage.preview.confirmButton;
	await expect(confirm).toBeEnabled();
	const imported = waitForApiResponse(page, 'POST', '/records/import');
	await confirm.click();
	expect((await imported).ok()).toBe(true);
	await page.waitForURL('/');
	await expect(page.getByRole('alertdialog')).toHaveCount(0);
});

// CSV の取り込みと同じ確認画面を使うが、出し直しの状態は写真で記録のページが持つ (docs/import-export.md)。
test('取り込み内容の確認を出した後にほかで記録が足されたら、取り込まずに出し直し、確かめてから取り込める', async ({
	page
}) => {
	const photoPage = await openNotebookRows(page);
	await photoPage.reviewButton.click();
	const confirm = photoPage.preview.confirmButton;
	await expect(confirm).toBeEnabled();

	const yesterday = addDays(todayIn(await fetchTimeZone(page.request)), -1);
	await createRecordByApi(page.request, {
		localMeasuredAt: `${yesterday}T12:00`,
		systolic: 130,
		diastolic: 85,
		memo: 'ほかで足した記録'
	});
	const conflicted = waitForApiResponse(page, 'POST', '/records/import');
	await confirm.click();
	expect((await conflicted).status()).toBe(409);

	await expect(photoPage.preview.changedElsewhereNotice).toBeVisible();
	await expect(photoPage.preview.row('ほかで足した記録')).toContainText(
		ja.import_preview_status_removed
	);

	const imported = waitForApiResponse(page, 'POST', '/records/import');
	await confirm.click();
	expect((await imported).ok()).toBe(true);
	await page.waitForURL('/');
});

// 脈拍も読み取った値として確かめる対象なので、上/下と同じ大きさで見せる (docs/import-export.md)。
test('読み取った記録の一覧では、脈拍の値を上/下と同じ大きさ・太さで見せ、「脈拍」の文字だけ小さく添える', async ({
	page
}) => {
	const photoPage = await openNotebookRows(page);
	const label = photoPage.rows.items.first().getByText(ja.record_pulse_label, { exact: true });
	const styles = await label.evaluate((element) => {
		const value = element.parentElement!;
		const line = value.parentElement!;
		const style = (target: Element) => {
			const { fontSize, fontWeight } = getComputedStyle(target);
			return { fontSize, fontWeight };
		};
		return {
			label: style(element),
			value: style(value),
			line: style(line),
			text: value.textContent
		};
	});
	expect(styles.text).toContain('70');
	expect(styles.value).toEqual(styles.line);
	expect(styles.label.fontSize).not.toBe(styles.value.fontSize);
});

// 取れてから差し込むと「手動で入力...」が押す直前に縮み、取れないと黙って消えるため。
test('ホーム画面の「写真で記録...」は、読み取りの状態を待たずに出し、取れなくても消さず、無効と分かれば消す', async ({
	page
}) => {
	type Reply = { status: number; json: unknown };
	// 応答を止めておき、出す時を決める。止めた順に並ぶ。
	const held: ((reply: Reply) => void)[] = [];
	await page.route('**/api/v1/ocr/status', async (route) => {
		await route.fulfill(await new Promise<Reply>((resolve) => held.push(resolve)));
	});
	async function reply(index: number, body: Reply) {
		await expect.poll(() => held.length).toBeGreaterThan(index);
		const responded = waitForApiResponse(page, 'GET', '/ocr/status');
		held[index](body);
		await responded;
	}
	const home = new HomePage(page);
	await home.open();
	await expect(home.photoButton).toBeVisible();

	await reply(0, { status: 500, json: { error: { message: 'x' } } });
	// 消えないことを確かめるので、画面が応答を受けて描き直すのを待ってから見る。
	await page.evaluate(
		() => new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve)))
	);
	await expect(home.photoButton).toBeVisible();

	await page.reload();
	await expect(home.photoButton).toBeVisible();
	await reply(1, {
		status: 200,
		json: { enabled: false, quotas: null, quotaLow: false, topupAvailable: false }
	});
	await expect(home.photoButton).toHaveCount(0);
});

// 選んだ写真はページの中だけに持って渡すので、移るときに読み込み直すと消える (docs/ocr.md)。
test('ホーム画面を開いた後にサーバーへ届かなくなっても、選んだ写真を写真で記録のページへ渡す', async ({
	page
}) => {
	await mockOcrStatus(page);
	const photo = await samplePhoto(page, {
		kind: 'monitor',
		values: { systolic: 120, diastolic: 80, pulse: 70 }
	});
	// 開いたときに読み込むコードが届き終わるまで待ち、その後のコードの取得をすべて断る
	// (サーバーが止まった・起動中のときと同じく、移るときにコードを取れない)。届き終わったかは、
	// ページごとのコード (`nodes/`) が4つ (ルートの layout・エラー画面・ホーム画面・先読みした写真で記録) 届いたかで見る。
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
	const home = new HomePage(page);
	await home.open();
	await expect.poll(() => nodes.size >= 4 && inFlight === 0).toBe(true);
	blocked = true;

	const photoPage = await home.choosePhoto(photo);
	await expect(photoPage.readButton).toBeVisible();
	await expect(photoPage.pickPhotoButton).toHaveCount(0);
});

test('写真が無いまま開くと「写真を選ぶ」を出し、選べば読み取る', async ({ page }) => {
	const values = { systolic: 120, diastolic: 80, pulse: 70 };
	await mockOcrStatus(page);
	await mockOcrExtract(page, { json: monitorResult(values) });
	const photoPage = new PhotoPage(page);
	await photoPage.open();
	await photoPage.pickPhoto(await samplePhoto(page, { kind: 'monitor', values }));
	await expect(photoPage.fields.systolic).toHaveValue('120');
});

// 第三者の AI に送る前に、送り先を示して明示の許可を得る (docs/ocr.md)。
test('同意していなければ、読み取る前に送り先を示して同意を求め、同意すると読み取る', async ({
	page
}) => {
	const values = { systolic: 120, diastolic: 80, pulse: 70 };
	await mockOcrStatus(page, { consented: false });
	await mockOcrExtract(page, { json: monitorResult(values) });
	const home = new HomePage(page);
	await home.open();
	const photoPage = await home.pickPhoto(await samplePhoto(page, { kind: 'monitor', values }));
	await expect(photoPage.consent.recipient).toBeVisible();
	await expect(photoPage.fields.systolic).toBeHidden();

	const consented = waitForApiResponse(page, 'POST', '/ocr/consent');
	await photoPage.consent.agreeButton.click();
	expect((await consented).status()).toBe(204);
	await expect(photoPage.consent.root).toBeHidden();
	await expect(photoPage.fields.systolic).toHaveValue('120');
});

test('同意を求められて「使わない」を選ぶと、写真を送らずに数字で入力するシートを開く', async ({
	page
}) => {
	const values = { systolic: 120, diastolic: 80, pulse: 70 };
	await mockOcrStatus(page, { consented: false });
	await mockOcrExtract(page, { json: monitorResult(values) });
	let extractCalls = 0;
	page.on('request', (request) => {
		if (request.method() === 'POST' && new URL(request.url()).pathname === '/api/v1/ocr') {
			extractCalls += 1;
		}
	});
	const home = new HomePage(page);
	await home.open();
	const photoPage = await home.pickPhoto(await samplePhoto(page, { kind: 'monitor', values }));
	await photoPage.consent.declineButton.click();
	await page.waitForURL('/');
	await expect(home.addSheet.root).toBeVisible();
	expect(extractCalls).toBe(0);
});

// 誤って選んだ写真で読み取りの枠を使わないよう、「読み取る」を押すまで読み取らない (docs/ocr.md)。
test('写真を選んでも「読み取る」を押すまで読み取らず、選び直した写真を読み取る', async ({
	page
}) => {
	const values = { systolic: 120, diastolic: 80, pulse: 70 };
	await mockOcrStatus(page);
	await mockOcrExtract(page, { json: monitorResult(values) });
	let extractCalls = 0;
	page.on('request', (request) => {
		if (request.method() === 'POST' && new URL(request.url()).pathname === '/api/v1/ocr') {
			extractCalls++;
		}
	});
	const home = new HomePage(page);
	await home.open();
	const photoPage = await home.choosePhoto(await samplePhoto(page, { kind: 'monitor', values }));
	await expect(photoPage.photoPreview.image).toBeVisible();
	await expect(photoPage.readButton).toBeVisible();

	await chooseFile(
		page,
		photoPage.retakeButton,
		await samplePhoto(page, { kind: 'monitor', values })
	);
	await expect(photoPage.readButton).toBeVisible();
	expect(extractCalls).toBe(0);

	await photoPage.readButton.click();
	await expect(photoPage.fields.systolic).toHaveValue('120');
	expect(extractCalls).toBe(1);
});

// 読み取りの枠の残りは、読み取りの画面の上に、今使っている枠を量によらず同じ枠 (見出し・ゲージ・割合) で出す。
// 買い足しは残りの量によらず出し、少なければ注意の色、使い切りは見出しと文言を替える。無制限は出さない (docs/payments.md)。
test('写真で記録のページに、今使っている枠の残りを割合の枠で出し、買い足しを量によらず出す。使い切りなら使い切りの文言にし、無制限なら出さない', async ({
	page
}) => {
	const photoPage = new PhotoPage(page);

	await mockOcrStatus(page, { quotas: [freeQuota(60)], topupAvailable: true });
	await photoPage.open();
	await expect(photoPage.freeQuotaTitle).toBeVisible();
	await expect(photoPage.quotaRemaining(60)).toBeVisible();
	await expect(photoPage.topupButton).toBeVisible();
	await expect(photoPage.otherQuotasLink).toHaveCount(0);

	await mockOcrStatus(page, { quotas: [freeQuota(20)], quotaLow: true, topupAvailable: true });
	await photoPage.open();
	await expect(photoPage.quotaRemaining(20)).toBeVisible();
	await expect(photoPage.topupButton).toBeVisible();

	await mockOcrStatus(page, { quotas: [freeQuota(0)], quotaLow: true });
	await photoPage.open();
	await expect(photoPage.quotaExhaustedTitle).toBeVisible();
	await expect(photoPage.quotaExhaustedManualNote).toBeVisible();
	await expect(photoPage.quotaRemaining(0)).toBeVisible();
	await expect(photoPage.pickPhotoButton).toHaveCount(0);
	await expect(photoPage.freeQuotaTitle).toHaveCount(0);

	await mockOcrStatus(page, { quotas: null });
	await photoPage.open();
	await expect(photoPage.freeQuotaTitle).toHaveCount(0);
});

// 買い足した枠を先に使う。今使っている枠だけを出し、ほかにも枠が残っていれば設定の一覧へ案内する。
test('買い足した枠を使う間は、その枠の残りを出し、ほかの枠が残っていれば設定の一覧へ案内する', async ({
	page
}) => {
	const photoPage = new PhotoPage(page);
	await mockOcrStatus(page, {
		quotas: [paidQuota(80, '2026-09-27T00:00:00.000Z'), freeQuota(100)]
	});
	await photoPage.open();
	await expect(photoPage.paidQuotaTitle).toBeVisible();
	await expect(photoPage.quotaRemaining(80)).toBeVisible();
	await expect(photoPage.freeQuotaTitle).toHaveCount(0);
	await photoPage.otherQuotasLink.click();
	await page.waitForURL('/settings#ocr');
});

// 使い切った後は読み取れないので、ホーム画面で写真を選んで開いても「読み取る」を出さず、数字で入力する導線を出す。
test('残りが0%のとき、ホーム画面で写真を選んで開いても「読み取る」を出さず、「数字で入力する」を出す', async ({
	page
}) => {
	const values = { systolic: 120, diastolic: 80, pulse: 70 };
	await mockOcrStatus(page, { quotas: [freeQuota(0)], quotaLow: true });
	const home = new HomePage(page);
	await home.open();
	const photoPage = await home.choosePhoto(await samplePhoto(page, { kind: 'monitor', values }));
	await expect(photoPage.quotaExhaustedTitle).toBeVisible();
	await expect(photoPage.readButton).toHaveCount(0);
	await expect(photoPage.retakeButton).toHaveCount(0);

	await expect((await photoPage.enterManually()).addSheet.root).toBeVisible();
});

// 読み取りが無効なら、写真を選んでも読み取れない。枠の残りも買い足しも出さない。
test('読み取りが無効なら、直接開いても「写真を選ぶ」と買い足しを出さず、「数字で入力する」を出す', async ({
	page
}) => {
	await mockOcrStatus(page, {
		enabled: false,
		quotas: [freeQuota(10)],
		quotaLow: true,
		topupAvailable: true
	});
	const photoPage = new PhotoPage(page);
	await photoPage.open();
	await expect(photoPage.ocrDisabledMessage).toBeVisible();
	await expect(photoPage.pickPhotoButton).toHaveCount(0);
	await expect(photoPage.topupButton).toHaveCount(0);
	await expect(photoPage.freeQuotaTitle).toHaveCount(0);

	await expect((await photoPage.enterManually()).addSheet.root).toBeVisible();
});

// 読み取りの途中で枠を使い切ったら、残量の取り直しを待たずに使い切りの枠を出し、同じことを失敗の文でも重ねない。
test('読み取りで無料枠の使い切り (429) を受けたら、使い切りの枠と「数字で入力する」だけを出す', async ({
	page
}) => {
	const values = { systolic: 120, diastolic: 80, pulse: 70 };
	await mockOcrStatus(page, { quotas: [freeQuota(60)] });
	await mockOcrExtract(page, errorResponse(429, 'ocr_budget_exhausted'));
	const photoPage = new PhotoPage(page);
	await photoPage.open();
	await photoPage.pickPhoto(await samplePhoto(page, { kind: 'monitor', values }));
	await expect(photoPage.quotaExhaustedTitle).toBeVisible();
	await expect(photoPage.errorMessage(ja.error_ocr_budget_exhausted)).toHaveCount(0);
	await expect(photoPage.againButton).toHaveCount(0);
	await expect(photoPage.manualEntryButton.first()).toBeVisible();
});

test('読み取りに失敗したら「もう一度」に加えて、「数字で入力する」と使い方のページへのリンクを出す', async ({
	page
}) => {
	const values = { systolic: 120, diastolic: 80, pulse: 70 };
	await mockOcrStatus(page);
	await mockOcrExtract(page, errorResponse(502, 'ocr_upstream_error'));
	const photoPage = new PhotoPage(page);
	await photoPage.open();
	await photoPage.pickPhoto(await samplePhoto(page, { kind: 'monitor', values }));
	await expect(photoPage.errorMessage(ja.error_ocr_upstream_error)).toBeVisible();
	await expect(photoPage.againButton).toBeVisible();
	await expect(photoPage.troubleHelpLink).toHaveAttribute('href', '/help/trouble-photo');

	await expect((await photoPage.enterManually()).addSheet.root).toBeVisible();
});

test('血圧値を読み取れなかったら、写っていたものに応じた案内を出し、Gemini の補足の文はそのまま出さない', async ({
	page
}) => {
	const notes = 'The display shows the current time.';
	await mockOcrStatus(page);
	await mockOcrExtract(page, { json: { ...noValuesResult(), notes } });
	const photoPage = new PhotoPage(page);
	await photoPage.open();
	const photo = await samplePhoto(page, { kind: 'monitor' });
	await photoPage.pickPhoto(photo);
	await expect(photoPage.noValuesMessage).toBeVisible();
	await expect(photoPage.noValuesHint('monitor_none')).toBeVisible();
	await expect(page.getByText(notes)).toBeHidden();

	await mockOcrExtract(page, { json: { ...noValuesResult(), kind: 'unknown', notes } });
	await photoPage.retake(photo);
	await expect(photoPage.noValuesHint('unknown')).toBeVisible();
	await expect(page.getByText(notes)).toBeHidden();
});

// 「低」だけ案内を替え、写真と見比べるよう促す (docs/ocr.md)。
test('血圧計の読み取りで信頼度が「低」なら、写真と見比べるよう案内する', async ({ page }) => {
	const values = { systolic: 120, diastolic: 80, pulse: 70 };
	const result = monitorResult(values);
	result.reading!.confidence = 0.5;
	const photoPage = await openPhotoPageFromHome(page, result, { kind: 'monitor', values });
	await expect(photoPage.reviewNote(ja.photo_page_review_note_low)).toBeVisible();
	await expect(photoPage.reviewNote(ja.photo_page_review_note)).toHaveCount(0);

	await mockOcrExtract(page, { json: monitorResult(values) });
	await photoPage.retake(await samplePhoto(page, { kind: 'monitor', values }));
	await expect(photoPage.reviewNote(ja.photo_page_review_note)).toBeVisible();
	await expect(photoPage.reviewNote(ja.photo_page_review_note_low)).toHaveCount(0);
});

// 昨日撮った写真を後から読み取っても、測った日時に近い値が入るように (docs/ocr.md)。
test('血圧計の写真の撮影日時 (Exif) を測定日時の既定値にし、読めなければ現在時刻にする', async ({
	page
}) => {
	const values = { systolic: 120, diastolic: 80, pulse: 70 };
	const today = todayIn(await fetchTimeZone(page.request));
	const yesterday = addDays(today, -1);
	const photoPage = await openPhotoPageFromHome(
		page,
		monitorResult(values),
		{ kind: 'monitor', values },
		{ dateTimeOriginal: `${yesterday.replaceAll('-', ':')} 07:15:42` }
	);
	await expect(photoPage.fields.date).toHaveValue(yesterday);
	await expect(photoPage.fields.time).toHaveValue('07:15');

	// Exif の無い写真に選び直すと、現在時刻に戻る。
	await photoPage.retake(await samplePhoto(page, { kind: 'monitor', values }));
	await expect(photoPage.fields.date).toHaveValue(today);
});

test('血圧計の読み取り結果は、ページの中のフォームで直してそのまま登録でき、登録するとホーム画面へ戻って、その記録の週を出す', async ({
	page
}) => {
	const today = todayIn(await fetchTimeZone(page.request));
	const pastDay = addDays(today, -60);
	// 最新の記録を今日にして、ホーム画面の初期表示 (最新の週) と登録した週を分ける。
	await createRecordByApi(page.request, {
		localMeasuredAt: `${today}T00:00`,
		systolic: 130,
		diastolic: 85,
		memo: '今日の記録'
	});
	const values = { systolic: 120, diastolic: 80, pulse: 70 };
	const { fields, createButton: submit } = await openPhotoPageFromHome(
		page,
		monitorResult(values),
		{ kind: 'monitor', values }
	);
	await expect(fields.systolic).toHaveValue('120');

	// 登録できない値は、記録フォームと同じく欄の下に理由を出し、送信しない。
	await fields.systolic.fill('70');
	await submit.click();
	await expect(fields.systolic).toHaveAttribute('aria-invalid', 'true');

	// 昔の写真を後から読み取った場合。ホーム画面の初期表示 (最新の週) には出ない日。
	await fields.date.fill(pastDay);
	await fields.fill({ systolic: '118', memo: '写真から登録' });
	const created = waitForApiResponse(page, 'POST', '/records');
	await submit.click();
	const response = await created;
	expect(response.status()).toBe(201);
	expect(response.request().postDataJSON()).toMatchObject({
		systolic: 118,
		diastolic: 80,
		pulse: 70,
		memo: '写真から登録'
	});
	await page.waitForURL('/');
	await expect(page.getByRole('alertdialog')).toHaveCount(0);
	const home = new HomePage(page);
	await expect(home.periodNav.thisWeekButton).toHaveText(weekLabel(pastDay));
	await expect(home.recordCard('写真から登録').root).toBeVisible();
});

// 同じ写真を2回登録しかけたのに気づかせる。
test('血圧計の読み取りで、同じ日時・同じ値の記録がすでにあれば、登録する前に確かめる', async ({
	page
}) => {
	const values = { systolic: 120, diastolic: 80, pulse: 70 };
	const yesterday = addDays(todayIn(await fetchTimeZone(page.request)), -1);
	await createRecordByApi(page.request, { localMeasuredAt: `${yesterday}T07:15`, ...values });
	const photoPage = await openPhotoPageFromHome(
		page,
		monitorResult(values),
		{ kind: 'monitor', values },
		{ dateTimeOriginal: `${yesterday.replaceAll('-', ':')} 07:15:42` }
	);
	const { sameRecord } = photoPage;
	let posted = 0;
	page.on('request', (request) => {
		if (request.method() === 'POST' && new URL(request.url()).pathname === '/api/v1/records') {
			posted += 1;
		}
	});

	// やめれば登録せず、ページに留まる。
	await photoPage.createButton.click();
	await expect(sameRecord.root).toBeVisible();
	await sameRecord.cancelButton.click();
	await expect(sameRecord.root).toHaveCount(0);
	await expect(photoPage.createButton).toBeEnabled();
	expect(posted).toBe(0);

	// 「登録する」を選べば、同じ記録をもう1件登録する。
	const created = waitForApiResponse(page, 'POST', '/records');
	await photoPage.createButton.click();
	await sameRecord.confirmButton.click();
	expect((await created).status()).toBe(201);
	await page.waitForURL('/');
});

// 写真の拡大 (全画面の photo-lightbox.svelte と、ページの写真の枠の photo-preview.svelte)。指の操作は Playwright のマウスとホイール、ピンチは CDP のタッチで送る。
test.describe('写真の拡大', () => {
	/** ダブルタップとみなす間隔 (photo-zoom.svelte.ts の DOUBLE_TAP_MS) より長く待つ時間。 */
	const AFTER_DOUBLE_TAP_WINDOW_MS = 400;

	/** 血圧計の読み取り結果を出し、プレビューから拡大表示を開く。 */
	async function openLightbox(page: Page) {
		const values = { systolic: 120, diastolic: 80, pulse: 70 };
		const photoPage = await openPhotoPageFromHome(page, monitorResult(values), {
			kind: 'monitor',
			values
		});
		const { openButton } = photoPage.photoPreview;
		const { root: lightbox, photo, closeButton } = await photoPage.photoPreview.openLightbox();
		// 開く動きの間は、画面への操作がすべて <html> に届き、写真には届かない。動きが終わるまで待つ。
		await expect
			.poll(() =>
				photo.evaluate((element) => {
					const rect = element.getBoundingClientRect();
					const hit = document.elementFromPoint(rect.x + rect.width / 2, rect.y + rect.height / 2);
					return hit === element;
				})
			)
			.toBe(true);
		return {
			photoPage,
			openButton,
			lightbox,
			photo,
			closeButton
		};
	}

	/** 写真に指定した拡大率と移動量。表示中の値 (getComputedStyle) ではなく指定した値を読む。
	 * ダブルタップの拡大は動きを付けるため、表示中の値は直後だとまだ変わっておらず、拡大しなかったことを確かめられない。
	 * 拡大は指の位置を中心にするので、座標の端数による 1px 未満の差が出る。小数第1位で丸める。 */
	function photoTransform(photo: Locator) {
		return photo.evaluate((element) => {
			const matrix = new DOMMatrixReadOnly(element.style.transform);
			const round = (value: number) => Math.round(value * 10) / 10 || 0;
			return { scale: round(matrix.a), x: round(matrix.e), y: round(matrix.f) };
		});
	}

	async function photoCenter(photo: Locator) {
		const box = await photo.boundingBox();
		if (!box) throw new Error('写真が表示されていない');
		return { x: box.x + box.width / 2, y: box.y + box.height / 2 };
	}

	test('写真の枠の拡大ボタンで全画面で出し、ダブルタップで拡大・元に戻せ、閉じる・Esc・戻る操作のどれでも閉じる', async ({
		page
	}) => {
		const { photoPage, openButton, lightbox, photo, closeButton } = await openLightbox(page);
		await expect(photoPage.lightbox.hint).toBeVisible();

		// ダブルタップの拡大・縮小は動きで見せる。
		await photo.dblclick();
		await expect(photo).toHaveCSS('transition-duration', '0.3s');
		await expect.poll(() => photoTransform(photo)).toEqual({ scale: 2.5, x: 0, y: 0 });
		await page.waitForTimeout(AFTER_DOUBLE_TAP_WINDOW_MS);
		await photo.dblclick();
		await expect.poll(() => photoTransform(photo)).toEqual({ scale: 1, x: 0, y: 0 });

		await closeButton.click();
		await expect(lightbox).toBeHidden();
		await expect(page).toHaveURL('/record/photo');
		// 開閉の動きのために付けた名前を、終わったら外す (残ると次の動きで名前が重なり、動かなくなる)。
		await expect(photoPage.photoPreview.view).toHaveCSS('view-transition-name', 'none');

		await openButton.click();
		await expect(lightbox).toBeVisible();
		await page.keyboard.press('Escape');
		await expect(lightbox).toBeHidden();
		await expect(page).toHaveURL('/record/photo');

		// 端末の戻る操作は、ページを離れずに拡大表示だけを閉じる。
		await openButton.click();
		await expect(lightbox).toBeVisible();
		await page.goBack();
		await expect(lightbox).toBeHidden();
		await expect(page).toHaveURL('/record/photo');
		await expect(photoPage.fields.systolic).toHaveValue('120');
	});

	/** 血圧計の読み取り結果を出し、ページの写真の枠 (`preview`) と、その中の写真を返す。 */
	async function openPreview(page: Page) {
		const values = { systolic: 120, diastolic: 80, pulse: 70 };
		const photoPage = await openPhotoPageFromHome(page, monitorResult(values), {
			kind: 'monitor',
			values
		});
		const preview = photoPage.photoPreview;
		const photo = preview.image;
		await expect(photo).toBeVisible();
		return { photoPage, preview, photo };
	}

	/** 枠の中でダブルタップしたときの倍率 (横幅いっぱいの 1.2 倍)。photoTransform と同じく小数第1位で丸める。 */
	async function frameZoomScale(preview: PhotoPreview) {
		const frameWidth = await preview.view.evaluate((element) => element.clientWidth);
		const imageWidth = await preview.image.evaluate((element: HTMLElement) => element.offsetWidth);
		return Math.round(Math.max(1, frameWidth / imageWidth) * 1.2 * 10) / 10;
	}

	/** 写真の枠の中で、今の倍率で写真を動かせる範囲 (写真の端が枠の端に来るまで)。photoTransform と同じく小数第1位で丸める。 */
	async function panLimit(preview: PhotoPreview) {
		const frame = await preview.view.evaluate((element) => ({
			width: element.clientWidth,
			height: element.clientHeight
		}));
		const image = await preview.image.evaluate((element: HTMLElement) => ({
			width: element.offsetWidth,
			height: element.offsetHeight,
			scale: new DOMMatrixReadOnly(element.style.transform).a
		}));
		const limit = (size: number, box: number) =>
			Math.round(Math.max(0, (size * image.scale - box) / 2) * 10) / 10;
		return { x: limit(image.width, frame.width), y: limit(image.height, frame.height) };
	}

	// 写真を見ながら数値を確かめられるよう、写真の枠は見出しの帯と一緒に上に残す (docs/ocr.md)。
	test('ページの写真の枠は、スクロールしても見出しの帯と一緒に上に残る', async ({ page }) => {
		await page.setViewportSize({ width: 412, height: 560 });
		const { photoPage } = await openPreview(page);
		const before = await settledBox(photoPage.photoPreview.openButton);

		// 指でのスクロールに近いホイールで、枠の外 (フォームの上) から動かす。
		await page.mouse.move(200, 500);
		await page.mouse.wheel(0, 600);
		await expect.poll(() => page.evaluate(() => scrollY)).toBeGreaterThan(0);
		expect(await settledBox(photoPage.photoPreview.openButton)).toEqual(before);
	});

	test('ページの写真の枠は拡大して開き、ドラッグで写真の端まで動かせ、ダブルタップで全体と行き来できる', async ({
		page
	}) => {
		const { preview, photo } = await openPreview(page);
		const center = await photoCenter(photo);

		// 横幅いっぱいより少し大きく (左右が少しはみ出すくらいに) 拡大して開く。
		const scale = await frameZoomScale(preview);
		await expect.poll(() => photoTransform(photo)).toEqual({ scale, x: 0, y: 0 });

		// 写真の端より先へは動かない。
		const limit = await panLimit(preview);
		await page.mouse.move(center.x, center.y);
		await page.mouse.down();
		await page.mouse.move(center.x + 1000, center.y + 1000, { steps: 5 });
		await page.mouse.up();
		await expect.poll(() => photoTransform(photo)).toEqual({ scale, x: limit.x, y: limit.y });

		await photo.dblclick();
		await expect.poll(() => photoTransform(photo)).toEqual({ scale: 1, x: 0, y: 0 });
		await page.waitForTimeout(AFTER_DOUBLE_TAP_WINDOW_MS);
		await photo.dblclick();
		await expect.poll(() => photoTransform(photo)).toEqual({ scale, x: 0, y: 0 });
	});

	test('ページの写真の枠の中で、2本指で広げると、指の間の所を中心に拡大する', async ({ page }) => {
		const { photo } = await openPreview(page);
		// 拡大して開くので、全体の表示に戻してから広げる。
		await expect.poll(async () => (await photoTransform(photo)).scale).toBeGreaterThan(1);
		await photo.dblclick();
		await expect.poll(() => photoTransform(photo)).toEqual({ scale: 1, x: 0, y: 0 });
		const center = await photoCenter(photo);
		const cdp = await page.context().newCDPSession(page);
		// 中心から下へずらした所を挟む2本の指。
		const at = { x: center.x, y: center.y + 20 };
		const fingers = (half: number) => [
			{ x: at.x - half, y: at.y, id: 0 },
			{ x: at.x + half, y: at.y, id: 1 }
		];

		await cdp.send('Input.dispatchTouchEvent', { type: 'touchStart', touchPoints: fingers(20) });
		for (const half of [30, 40, 50]) {
			await cdp.send('Input.dispatchTouchEvent', { type: 'touchMove', touchPoints: fingers(half) });
		}
		await cdp.send('Input.dispatchTouchEvent', { type: 'touchEnd', touchPoints: [] });

		// 2.5 倍にしても指の間の所が動かないよう、中心からのずれの 1.5 倍だけ逆へ動かす。
		const transform = await photoTransform(photo);
		expect(transform.scale).toBeCloseTo(2.5, 2);
		expect(transform.x).toBe(0);
		expect(transform.y).toBeCloseTo(-30, 0);
	});

	// 枠は画面の上に留まるので、ホイールだけで拡大すると PC でページを送れなくなる。
	test('ページの写真の枠の上では、Ctrl + ホイールで拡大し、ホイールだけならページをスクロールする', async ({
		page
	}) => {
		await page.setViewportSize({ width: 412, height: 560 });
		const { preview, photo } = await openPreview(page);
		const scale = await frameZoomScale(preview);
		await expect.poll(() => photoTransform(photo)).toEqual({ scale, x: 0, y: 0 });
		const center = await photoCenter(photo);
		await page.mouse.move(center.x, center.y);

		await page.mouse.wheel(0, 300);
		await expect.poll(() => page.evaluate(() => scrollY)).toBeGreaterThan(0);
		expect(await photoTransform(photo)).toEqual({ scale, x: 0, y: 0 });

		await page.keyboard.down('Control');
		await page.mouse.wheel(0, -50);
		await page.keyboard.up('Control');
		await expect.poll(async () => (await photoTransform(photo)).scale).toBeGreaterThan(scale + 0.1);
	});

	// 入力エラーの欄へ送るスクロールで、欄が帯の下に隠れないようにする。隠れないこと自体は、Chromium が
	// 画面外の欄への focus() で中央へ送るため e2e では差が出ない。余白の付け外しを確かめる。
	test('見出しの帯 (写真の枠を含む) の高さをスクロールの余白にし、ページを離れると外す', async ({
		page
	}) => {
		const { photoPage } = await openPreview(page);
		const scrollPaddingTop = () =>
			page.evaluate(() => document.documentElement.style.scrollPaddingTop);
		const frameBox = await settledBox(photoPage.photoPreview.openButton);
		await expect
			.poll(async () => parseFloat(await scrollPaddingTop()))
			.toBeGreaterThanOrEqual(frameBox.y + frameBox.height);

		await photoPage.back();
		await expect.poll(scrollPaddingTop).toBe('');
	});

	// シートの枠は小さいので、全体より一部を大きく見せる (docs/ocr.md)。
	test('行を直すシートの写真の枠は、横幅いっぱいより少し大きく開き、ダブルタップで全体と行き来できる', async ({
		page
	}) => {
		const photoPage = await openNotebookRows(page);
		const sheet = await photoPage.rows.openRow(photoPage.rows.items.first());
		const photo = sheet.photoPreview.image;
		await expect(photo).toBeVisible();
		const zoomed = await frameZoomScale(sheet.photoPreview);
		await expect.poll(() => photoTransform(photo)).toEqual({ scale: zoomed, x: 0, y: 0 });

		// ダブルタップで全体に戻し、もう一度で最初の見え方に戻る。
		await photo.dblclick();
		await expect.poll(() => photoTransform(photo)).toEqual({ scale: 1, x: 0, y: 0 });
		await page.waitForTimeout(AFTER_DOUBLE_TAP_WINDOW_MS);
		await photo.dblclick();
		await expect.poll(() => photoTransform(photo)).toEqual({ scale: zoomed, x: 0, y: 0 });
	});

	test('行を直すシートの写真の枠から開いた全画面は、中を触ってもシートを閉じず、戻る操作・Esc で閉じてシートは残る', async ({
		page
	}) => {
		const photoPage = await openNotebookRows(page);
		const { lightbox } = photoPage;
		const firstRow = photoPage.rows.items.first();
		const sheet = await photoPage.rows.openRow(firstRow);
		await sheet.fields.systolic.fill('118');

		await sheet.photoPreview.openLightbox();
		// 全画面の中の操作を、下のシートの外側のクリックとして扱わない。
		await lightbox.photo.click();
		await lightbox.hint.click();
		await page.goBack();
		await expect(lightbox.root).toBeHidden();
		await expect(sheet.root).toBeVisible();
		await expect(page).toHaveURL('/record/photo');

		await sheet.photoPreview.openLightbox();
		await page.keyboard.press('Escape');
		await expect(lightbox.root).toBeHidden();
		await expect(sheet.root).toBeVisible();

		await sheet.photoPreview.openLightbox();
		await lightbox.closeButton.click();
		await expect(lightbox.root).toBeHidden();
		await expect(sheet.root).toBeVisible();

		// 閉じた後も直しかけの値が残り、そのまま書き戻せる。
		await expect(sheet.fields.systolic).toHaveValue('118');
		await sheet.updateButton.click();
		await expect(sheet.root).toBeHidden();
		await expect(firstRow).toContainText(/118\s*\//);
		await expect(page).toHaveURL('/record/photo');
	});

	test('閉じた後の「進む」では開き直さず、続く戻る操作1回でページを離れる', async ({ page }) => {
		const { lightbox, closeButton } = await openLightbox(page);

		await closeButton.click();
		await expect(lightbox).toBeHidden();

		// 拡大率やどこを見ていたかは履歴に持っていないので、開き直さず、開いた履歴から戻る。
		await page.goForward();
		await expect
			.poll(() => page.evaluate(() => history.state?.['sveltekit:states']?.photoLightbox ?? null))
			.toBeNull();
		await expect(lightbox).toBeHidden();
		await expect(page).toHaveURL('/record/photo');

		// 開いた履歴に留まっていると、ここでの戻る操作は同じ画面に戻るだけで空振りする。
		await page.goBack();
		await expect(page).toHaveURL('/');
	});

	test('1回のタップや、動かす操作を挟んだタップでは拡大しない', async ({ page }) => {
		const { photo } = await openLightbox(page);
		const center = await photoCenter(photo);

		await page.mouse.click(center.x, center.y);
		expect(await photoTransform(photo)).toEqual({ scale: 1, x: 0, y: 0 });

		// タップ → ドラッグ → タップを、ダブルタップの間隔の内に続けて送る。
		await page.waitForTimeout(AFTER_DOUBLE_TAP_WINDOW_MS);
		await page.mouse.click(center.x, center.y);
		await page.mouse.down();
		await page.mouse.move(center.x + 40, center.y, { steps: 4 });
		await page.mouse.up();
		await page.mouse.click(center.x + 40, center.y);
		expect(await photoTransform(photo)).toEqual({ scale: 1, x: 0, y: 0 });

		// 指を離した後の捕捉の解除でタップの記録を消さず、ダブルタップは効く。
		await page.waitForTimeout(AFTER_DOUBLE_TAP_WINDOW_MS);
		await photo.dblclick();
		await expect.poll(() => photoTransform(photo)).toEqual({ scale: 2.5, x: 0, y: 0 });
	});

	test('ホイールで拡大・縮小し、拡大中はドラッグで写真を動かせる', async ({ page }) => {
		const { photo } = await openLightbox(page);
		const center = await photoCenter(photo);

		await page.mouse.move(center.x, center.y);
		await page.mouse.wheel(0, -500);
		// 届くホイールの量は端末の画素密度で変わるので、倍率は決め打ちしない。
		await expect.poll(async () => (await photoTransform(photo)).scale).toBeGreaterThan(1.2);
		// 指やホイールで動かす間は、遅れて付いてこないよう動きを付けない。
		await expect(photo).toHaveCSS('transition-duration', '0s');

		// ホイールの拡大はカーソルの位置を中心にするので、座標の端数で 1px 未満ずれる。
		const zoomed = await photoTransform(photo);
		await page.mouse.down();
		await page.mouse.move(center.x + 40, center.y + 30, { steps: 4 });
		await page.mouse.up();
		await expect
			.poll(async () => {
				const { x, y } = await photoTransform(photo);
				return { x: Math.round(x - zoomed.x), y: Math.round(y - zoomed.y) };
			})
			.toEqual({ x: 40, y: 30 });

		// 等倍まで縮めると、動かした分も戻す。
		await page.mouse.wheel(0, 2000);
		await expect.poll(() => photoTransform(photo)).toEqual({ scale: 1, x: 0, y: 0 });
	});

	test('2本指で広げると拡大する', async ({ page }) => {
		const { photo } = await openLightbox(page);
		const center = await photoCenter(photo);
		const cdp = await page.context().newCDPSession(page);
		/** 中心から左右に `half` px ずつ離した2本の指。 */
		const fingers = (half: number) => [
			{ x: center.x - half, y: center.y, id: 0 },
			{ x: center.x + half, y: center.y, id: 1 }
		];

		await cdp.send('Input.dispatchTouchEvent', { type: 'touchStart', touchPoints: fingers(50) });
		for (const half of [75, 100, 125]) {
			await cdp.send('Input.dispatchTouchEvent', { type: 'touchMove', touchPoints: fingers(half) });
		}
		await cdp.send('Input.dispatchTouchEvent', { type: 'touchEnd', touchPoints: [] });

		// 指の間隔が 100px から 250px に広がったので 2.5 倍。
		await expect.poll(async () => (await photoTransform(photo)).scale).toBeCloseTo(2.5, 2);
	});

	// 開閉の動き (View Transitions) を使う経路と、動きを減らす設定で使わない経路の両方で確かめる。
	for (const reducedMotion of ['no-preference', 'reduce'] as const) {
		test.describe(`動きを減らす設定が ${reducedMotion}`, () => {
			test.use({ reducedMotion });

			test('閉じる操作が続けて届いても、ページに留まって開き直せる', async ({ page }) => {
				const { openButton, lightbox, closeButton } = await openLightbox(page);

				// 戻した履歴が届く前に2回目を押す (受けるとページまで戻る)。
				await closeButton.evaluate((button: HTMLElement) => {
					button.click();
					button.click();
				});
				await expect(lightbox).toBeHidden();
				await openButton.click();
				await expect(lightbox).toBeVisible();
				await expect(page).toHaveURL('/record/photo');
			});
		});
	}

	test('閉じる動きの途中で写真をタップしても開かず、動きが終われば開ける', async ({ page }) => {
		const { photoPage, openButton, lightbox, closeButton } = await openLightbox(page);
		// 途中の状態を確実に作るため、閉じる動きを長くする。
		await page.addStyleTag({
			content: '::view-transition-group(*) { animation-duration: 3s !important; }'
		});
		const preview = photoPage.photoPreview.view;

		await closeButton.click();
		await expect(lightbox).toBeHidden();
		// 枠の大きさに切り取った要素へ戻す (拡大した写真そのものへ戻すと、枠の外へはみ出して見える)。
		await expect(preview).toHaveCSS('view-transition-name', 'photo-lightbox');
		// 動きの間は操作が <html> に届くため、ボタンを直接押す。
		await openButton.evaluate((button: HTMLElement) => button.click());
		await expect(lightbox).toBeHidden();

		// 動きが終わると、付けた名前が外れる。
		await expect(preview).toHaveCSS('view-transition-name', 'none', { timeout: 10_000 });
		await openButton.evaluate((button: HTMLElement) => button.click());
		await expect(lightbox).toBeVisible();
	});
});
