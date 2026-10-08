// 記録の登録・編集・削除と、入力の誤り。共有の管理者で流す。
// 既存の環境の記録と見分けるため、テストが作る記録のメモには一意な値を入れる。
import type { Page, Response } from '@playwright/test';
import {
	createRecordByApi,
	deleteRecordByApi,
	fetchTimeZone,
	updateRecordByApi,
	type NewRecord
} from './api-helpers';
import { addDays, todayIn, weekLabel } from './date-helpers';
import { test, expect } from './fixtures';
import { isApiResponse, ja, uniqueId, waitForApiResponse } from './helpers';
import { HomePage } from './pages/home-page';

/** ホーム画面を開き、下部アクションバーから記録フォームを開く。 */
async function openAddSheet(page: Page) {
	const home = new HomePage(page);
	await home.open();
	return { home, sheet: await home.openAddSheet() };
}

test('手動で入力した記録が一覧に出る', async ({ page, adminRecordCleanup }) => {
	const memo = uniqueId('e2e-record');
	adminRecordCleanup.add(memo);
	const { home, sheet } = await openAddSheet(page);
	// 測定日時は既定の現在時刻のままにする。一覧の初期表示 (今週) に入るため。
	await sheet.fields.fill({ systolic: '120', diastolic: '80', pulse: '70', memo });

	const created = waitForApiResponse(page, 'POST', '/records');
	await sheet.createButton.click();
	expect((await created).status()).toBe(201);
	await expect(sheet.root).toBeHidden();

	const card = home.recordCard(memo).root;
	await expect(card).toBeVisible();
	await expect(card).toContainText(/120\s*\/\s*80/);
	await expect(card).toContainText(ja.record_pulse_with_value.replace('{pulse}', '70'));
});

test('表示中の期間の外の日付で登録すると、その日を含む週へ移って一覧に出す', async ({
	page,
	adminRecordCleanup
}) => {
	const memo = uniqueId('e2e-outside');
	adminRecordCleanup.add(memo);
	const pastDay = addDays(todayIn(await fetchTimeZone(page.request)), -60);
	const { home, sheet } = await openAddSheet(page);
	await sheet.fields.date.fill(pastDay);
	await sheet.fields.fill({ systolic: '121', diastolic: '81', memo });

	const created = waitForApiResponse(page, 'POST', '/records');
	await sheet.createButton.click();
	expect((await created).status()).toBe(201);
	await expect(sheet.root).toBeHidden();

	await expect(home.periodNav.thisWeekButton).toHaveText(weekLabel(pastDay));
	await expect(home.recordCard(memo).root).toBeVisible();
});

test('記録を編集できる', async ({ page, adminRecordCleanup }) => {
	const memo = uniqueId('e2e-edit');
	adminRecordCleanup.add(memo);
	const { id, home, sheet } = await openEditFormForNewRecord(page, memo);
	await sheet.fields.systolic.fill('132');
	const updated = waitForApiResponse(page, 'PUT', `/records/${id}`);
	await sheet.updateButton.click();
	expect((await updated).ok()).toBe(true);
	await expect(sheet.root).toBeHidden();
	await expect(home.recordCard(memo).root).toContainText(/132\s*\/\s*80/);
});

/**
 * 管理者の今日に記録を作り、ホームを開く。
 * 今日の 0:00 にするのは、朝早くに流しても未来の日時にならず、一覧の初期表示 (最新の記録のある週) に入るため。
 */
async function createTodayRecordAndOpenHome(page: Page, memo: string) {
	const record: NewRecord = {
		localMeasuredAt: `${todayIn(await fetchTimeZone(page.request))}T00:00`,
		systolic: 120,
		diastolic: 80,
		memo
	};
	const id = await createRecordByApi(page.request, record);
	const home = new HomePage(page);
	await home.open();
	return { id, record, home };
}

/** 管理者の今日に記録を作り、一覧のカードのゴミ箱から削除の確認を開く。 */
async function openDeleteForNewRecord(page: Page, memo: string) {
	const { id, record, home } = await createTodayRecordAndOpenHome(page, memo);
	return { id, record, home, dialog: await home.openDelete(memo) };
}

/** 管理者の今日に記録を作り、一覧のカードから編集のシートを開く。 */
async function openEditFormForNewRecord(page: Page, memo: string) {
	const { id, record, home } = await createTodayRecordAndOpenHome(page, memo);
	const sheet = await home.openRecord(memo);
	await expect(sheet.fields.systolic).toHaveValue('120');
	return { id, record, home, sheet };
}

test('一覧のカードのゴミ箱から、確かめて削除できる', async ({ page, adminRecordCleanup }) => {
	const memo = uniqueId('e2e-delete');
	adminRecordCleanup.add(memo);
	const { id, home, dialog } = await openDeleteForNewRecord(page, memo);
	await expect(dialog.root).toContainText(/120 \/ 80/);
	const deleted = waitForApiResponse(page, 'DELETE', `/records/${id}`);
	await dialog.deleteButton.click();
	expect((await deleted).ok()).toBe(true);
	await expect(dialog.root).toBeHidden();
	await expect(page.getByText(memo, { exact: true })).toHaveCount(0);
	// 押したゴミ箱はカードごと消えるので、フォーカスは一覧の見出しへ移る。
	await expect(home.heading).toBeFocused();
});

test('消すのに失敗したら、確認を開いたままにしてその場でやり直せる', async ({
	page,
	adminRecordCleanup
}) => {
	const memo = uniqueId('e2e-delete-failed');
	adminRecordCleanup.add(memo);
	const { id, dialog } = await openDeleteForNewRecord(page, memo);

	await page.route(`**/api/v1/records/${id}?*`, (route) =>
		route.request().method() === 'DELETE'
			? route.fulfill({ status: 500, json: { error: { message: 'x' } } })
			: route.continue()
	);
	await dialog.deleteButton.click();
	await expect(dialog.errorNotice).toBeVisible();
	await page.keyboard.press('Escape');
	await expect(dialog.errorNotice).toBeHidden();
	await expect(dialog.root).toBeVisible();

	await page.unrouteAll();
	const deleted = waitForApiResponse(page, 'DELETE', `/records/${id}`);
	await dialog.deleteButton.click();
	expect((await deleted).ok()).toBe(true);
	await expect(page.getByText(memo, { exact: true })).toHaveCount(0);
});

// 直す・消すまでの間にほかの端末で記録が変わったら、止めて最新の値を出し直す。
// ほかの端末の操作は、同じログイン状態の API で行う。
test('開いた後にほかで直された記録は、直しても上書きせず、最新の値を出し直してから直せる', async ({
	page,
	adminRecordCleanup
}) => {
	const memo = uniqueId('e2e-conflict-update');
	adminRecordCleanup.add(memo);
	const { id, record, home, sheet } = await openEditFormForNewRecord(page, memo);
	await updateRecordByApi(page.request, id, { ...record, systolic: 140 }, 1);

	await sheet.fields.systolic.fill('132');
	const conflicted = waitForApiResponse(page, 'PUT', `/records/${id}`);
	await sheet.updateButton.click();
	expect((await conflicted).status()).toBe(409);
	// 入力途中の値は捨て、最新の値で出し直す。
	await expect(sheet.changedElsewhereNotice).toBeVisible();
	await expect(sheet.fields.systolic).toHaveValue('140');
	await expect(page.getByRole('alertdialog')).toHaveCount(0);
	// 一覧も最新にする (古い版番号のまま開き直さないように)。
	await expect(home.recordCard(memo).root).toContainText(/140\s*\/\s*80/);

	// 出し直した値は開いた時点の値として扱うので、閉じても確かめない。
	await page.goBack();
	await expect(sheet.root).toBeHidden();
	await expect(page.getByRole('alertdialog')).toHaveCount(0);

	// 開き直すと案内は消え、最新の版から直せる。
	const reopened = await home.openRecord(memo);
	await expect(reopened.changedElsewhereNotice).toHaveCount(0);
	await reopened.fields.systolic.fill('132');
	const updated = waitForApiResponse(page, 'PUT', `/records/${id}`);
	await reopened.updateButton.click();
	expect((await updated).ok()).toBe(true);
	await expect(reopened.root).toBeHidden();
	await expect(home.recordCard(memo).root).toContainText(/132\s*\/\s*80/);
});

test('出し直した後のシートで、そのまま直せる', async ({ page, adminRecordCleanup }) => {
	const memo = uniqueId('e2e-conflict-retry');
	adminRecordCleanup.add(memo);
	const { id, record, home, sheet } = await openEditFormForNewRecord(page, memo);
	await updateRecordByApi(page.request, id, { ...record, systolic: 140 }, 1);

	await sheet.fields.systolic.fill('132');
	await sheet.updateButton.click();
	await expect(sheet.fields.systolic).toHaveValue('140');
	await sheet.fields.systolic.fill('134');
	const updated = waitForApiResponse(page, 'PUT', `/records/${id}`);
	await sheet.updateButton.click();
	expect((await updated).ok()).toBe(true);
	await expect(sheet.root).toBeHidden();
	await expect(home.recordCard(memo).root).toContainText(/134\s*\/\s*80/);
});

test('確認を開いた後にほかで直された記録は、消さずに一覧を最新にし、そのまま消し直せる', async ({
	page,
	adminRecordCleanup
}) => {
	const memo = uniqueId('e2e-conflict-delete');
	adminRecordCleanup.add(memo);
	const { id, record, home, dialog } = await openDeleteForNewRecord(page, memo);
	await updateRecordByApi(page.request, id, { ...record, systolic: 140 }, 1);

	const conflicted = waitForApiResponse(page, 'DELETE', `/records/${id}`);
	await dialog.deleteButton.click();
	expect((await conflicted).status()).toBe(409);
	// 消すかどうかは最新の値を見て決め直してもらう。
	await expect(dialog.errorNotice).toContainText(ja.record_delete_changed_elsewhere);
	await expect(home.recordCard(memo).root).toContainText(/140\s*\/\s*80/);
	await page.keyboard.press('Escape');
	await expect(dialog.errorNotice).toBeHidden();

	// 確認には最新の値を出し、そのまま消せる。
	const reopened = await home.openDelete(memo);
	await expect(reopened.root).toContainText(/140 \/ 80/);
	const deleted = waitForApiResponse(page, 'DELETE', `/records/${id}`);
	await reopened.deleteButton.click();
	expect((await deleted).ok()).toBe(true);
	await expect(page.getByText(memo, { exact: true })).toHaveCount(0);
});

test('開いた後にほかで消された記録を直すと、消されていると伝えてシートを閉じ、一覧から消す', async ({
	page,
	adminRecordCleanup
}) => {
	const memo = uniqueId('e2e-deleted-update');
	adminRecordCleanup.add(memo);
	const { id, sheet } = await openEditFormForNewRecord(page, memo);
	await deleteRecordByApi(page.request, id, 1);

	const response = waitForApiResponse(page, 'PUT', `/records/${id}`);
	await sheet.updateButton.click();
	expect((await response).status()).toBe(404);

	await expect(sheet.errorNotice).toContainText(ja.record_deleted_elsewhere);
	await expect(sheet.root).toBeHidden();
	await expect(page.getByText(memo, { exact: true })).toHaveCount(0);
});

test('確認を開いた後にほかで消された記録を消すと、消されていると伝えて一覧から消す', async ({
	page,
	adminRecordCleanup
}) => {
	const memo = uniqueId('e2e-deleted-delete');
	adminRecordCleanup.add(memo);
	const { id, dialog } = await openDeleteForNewRecord(page, memo);
	await deleteRecordByApi(page.request, id, 1);

	const response = waitForApiResponse(page, 'DELETE', `/records/${id}`);
	await dialog.deleteButton.click();
	expect((await response).status()).toBe(404);

	await expect(dialog.errorNotice).toContainText(ja.record_deleted_elsewhere);
	await expect(page.getByText(memo, { exact: true })).toHaveCount(0);
});

test('値域外や、上の血圧が下以下の値は登録できず、理由が出る', async ({
	page,
	adminRecordCleanup
}) => {
	// 誤って登録されてしまったら後片付けできるよう、一意なメモを入れて控える。
	const memo = uniqueId('e2e-invalid');
	adminRecordCleanup.add(memo);
	const created: Response[] = [];
	page.on('response', (res) => {
		if (isApiResponse(res, 'POST', '/records') && res.ok()) created.push(res);
	});
	const { sheet } = await openAddSheet(page);
	await sheet.fields.memo.fill(memo);

	// 値域は上の血圧 60-260 (`src/validation.rs`)。
	await sheet.fields.fill({ systolic: '300', diastolic: '80' });
	await sheet.createButton.click();
	await expect(
		sheet.root.getByText(
			ja.record_error_systolic_range.replace('{min}', '60').replace('{max}', '260'),
			{ exact: true }
		)
	).toBeVisible();
	await expect(sheet.fields.systolic).toHaveAttribute('aria-invalid', 'true');

	await sheet.fields.fill({ systolic: '80', diastolic: '90' });
	await sheet.createButton.click();
	await expect(
		sheet.root.getByText(ja.record_error_systolic_not_greater, { exact: true })
	).toBeVisible();

	// 明日以降の日付は登録しない。
	const tomorrow = addDays(todayIn(await fetchTimeZone(page.request)), 1);
	await sheet.fields.fill({ systolic: '120', diastolic: '80' });
	await sheet.fields.date.fill(tomorrow);
	await sheet.createButton.click();
	await expect(sheet.root.getByText(ja.record_error_future_date, { exact: true })).toBeVisible();
	await expect(sheet.fields.date).toHaveAttribute('aria-invalid', 'true');
	await expect(sheet.root).toBeVisible();
	expect(created).toHaveLength(0);
});

test('記録フォームに入力した後に閉じようとすると確かめ、とどまれば入力が残る', async ({ page }) => {
	const { sheet } = await openAddSheet(page);
	await sheet.fields.systolic.fill('120');

	// 端末の戻る操作。
	await page.goBack();
	const { discardConfirm } = sheet;
	await expect(discardConfirm.root).toBeVisible();
	await discardConfirm.stayButton.click();
	await expect(discardConfirm.root).toBeHidden();
	await expect(page).toHaveURL('/');
	await expect(sheet.fields.systolic).toHaveValue('120');

	// Escape でも確かめ、閉じればもう一度戻る操作をしても開き直さない。
	await page.keyboard.press('Escape');
	await expect(discardConfirm.root).toBeVisible();
	await discardConfirm.leaveButton.click();
	await expect(sheet.root).toBeHidden();
	await expect(page).toHaveURL('/');
});

test('記録フォームに入力していなければ、端末の戻る操作で確かめずに閉じる', async ({ page }) => {
	const { sheet } = await openAddSheet(page);
	await expect(sheet.fields.systolic).toBeVisible();

	await page.goBack();
	await expect(sheet.root).toBeHidden();
	await expect(page.getByRole('alertdialog')).toHaveCount(0);
	await expect(page).toHaveURL('/');
});

test('閉じた後の「進む」ではシートが開き直さず、開いた履歴にも留まらない', async ({ page }) => {
	const home = new HomePage(page);
	await home.open();
	// 履歴のどこにいるか。ホームの画面は同じなので、URL ではなく履歴の位置で比べる (e2e は Chromium の Navigation API)。
	const historyIndex = () =>
		page.evaluate(
			() =>
				(window as { navigation?: { currentEntry?: { index: number } } }).navigation?.currentEntry
					?.index
		);
	const before = await historyIndex();
	const sheet = await home.openAddSheet();
	await expect(sheet.fields.systolic).toBeVisible();
	await page.goBack();
	await expect(sheet.root).toBeHidden();

	// 入力していた値やどの記録を開いていたかは履歴に持っていないので、開き直さず、開いた履歴から戻る。
	// 留まると、次の戻る操作が同じ画面に戻るだけで空振りする。
	await page.goForward();
	await expect.poll(historyIndex).toBe(before);
	await expect(sheet.root).toBeHidden();
	await expect(page).toHaveURL('/');

	// 開き直しても、戻る操作1回で閉じる。
	await home.openAddSheet();
	await expect(sheet.fields.systolic).toBeVisible();
	await page.goBack();
	await expect(sheet.root).toBeHidden();
	await expect(page).toHaveURL('/');
});

test('必須の欄が空のままでは送れず、ブラウザの検証が止める', async ({ page }) => {
	const { sheet } = await openAddSheet(page);
	// 日時は既定で入っているので、血圧の欄だけを空にする。
	await sheet.fields.systolic.fill('');
	await sheet.fields.diastolic.fill('80');

	await sheet.createButton.click();

	// シートは開いたままで、空の欄が未入力として弾かれている (吹き出しは操作できないので validity で見る)。
	await expect(sheet.root).toBeVisible();
	await expect(sheet.fields.systolic).toHaveJSProperty('validity.valueMissing', true);
	await expect(sheet.fields.diastolic).toHaveJSProperty('validity.valueMissing', false);
	// ブラウザが送信の前に止めるので、送信してから出す自前のエラーには届かない。
	await expect(sheet.fields.systolic).not.toHaveAttribute('aria-invalid', 'true');
	await expect(
		sheet.root.getByText(ja.record_error_required.replace('{field}', ja.record_systolic_label), {
			exact: true
		})
	).toBeHidden();
});
