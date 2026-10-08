// ユーザー管理 (一覧・追加・凍結・削除の予約・パスワードの再設定・閲覧)。共有の管理者で流す。
// 操作する相手は、画面から追加したユーザーか、API で作った使い捨てユーザー (`managedUser`) にする。
import { createRecordByApi, fetchTimeZone, loginStatus } from './api-helpers';
import { todayIn } from './date-helpers';
import { test, expect, NO_LOGIN } from './fixtures';
import { ja, uniqueId, waitForApiResponse } from './helpers';
import { AdminUsersPage } from './pages/admin-users-page';
import { DialogResult } from './pages/dialog-result';
import { HomePage } from './pages/home-page';
import { uniquePassword } from './user-helpers';

/** 結果に差し替わったダイアログを閉じる。 */
async function closeResult(result: DialogResult) {
	await result.closeButton.click();
	await expect(result.root).toBeHidden();
}

test('画面からユーザーを追加すると、一覧に出てそのユーザーでログインできる', async ({
	page,
	adminUserCleanup,
	newLoggedOutRequest
}) => {
	const username = uniqueId('e2e-added');
	const password = uniquePassword();
	adminUserCleanup.add(username, password);

	const adminUsers = new AdminUsersPage(page);
	await adminUsers.open();
	const dialog = await adminUsers.openAddUser();
	await dialog.username.fill(username);
	await dialog.password.fill(password);
	await dialog.passwordConfirmation.fill(password);
	await dialog.submitButton.click();
	await expect(dialog.done.root).toContainText(
		ja.admin_users_add_done_description.replace('{username}', username)
	);
	await closeResult(dialog.done);

	await expect(adminUsers.userCard(username)).toContainText(ja.admin_users_status_active);
	// 管理者の `page` のログイン状態を替えないよう、別のコンテキストで試す。
	const request = await newLoggedOutRequest();
	try {
		expect(await loginStatus(request, username, password)).toBe(200);
	} finally {
		await request.dispose();
	}
});

test('凍結するとログインできず、削除の予約と取り消しを経て解除するとログインできる', async ({
	page,
	managedUser: user
}) => {
	const adminUsers = new AdminUsersPage(page);
	await adminUsers.open();
	const card = adminUsers.userCard(user.username);

	await adminUsers.freezeButton(user.username).click();
	await expect(card).toContainText(ja.admin_users_status_frozen);
	expect(await loginStatus(user.request, user.username, user.password)).toBe(403);

	// 削除は凍結中のユーザーにだけ出る。確認にはユーザー ID を入れる。
	const dialog = await adminUsers.openDeleteUser(user.username);
	await expect(dialog.submitButton).toBeDisabled();
	await dialog.confirmation.fill(user.username);
	await dialog.submitButton.click();
	await closeResult(dialog.scheduled);
	await expect(card).toContainText(ja.admin_users_status_deletion_scheduled);

	await adminUsers.cancelDeletionButton(user.username).click();
	await expect(card).toContainText(ja.admin_users_status_frozen);
	await expect(card).not.toContainText(ja.admin_users_status_deletion_scheduled);

	await adminUsers.unfreezeButton(user.username).click();
	await expect(card).toContainText(ja.admin_users_status_active);
	expect(await loginStatus(user.request, user.username, user.password)).toBe(200);
});

test('OCR上限を変えると一覧に反映され、ダイアログが閉じて画面を操作できる', async ({
	page,
	managedUser: user
}) => {
	const adminUsers = new AdminUsersPage(page);
	await adminUsers.open();
	const card = adminUsers.userCard(user.username);

	const dialog = await adminUsers.openOcrLimit(user.username);
	// 見出しの × ではなく、最初の入力欄にフォーカスが入る。
	await expect(dialog.budgetMode).toBeFocused();
	await dialog.budgetMode.selectOption('custom');
	await dialog.budget.fill('200');
	await dialog.submitButton.click();
	await expect(dialog.root).toBeHidden();
	await expect(card).toContainText(
		ja.admin_users_ocr_limit_summary.replace('{budget}', '200').replace('{spent}', '0')
	);

	// 閉じきらないと覆いが残って押せないため、開き直せることまで確かめる。
	const reopened = await adminUsers.openOcrLimit(user.username);
	await expect(reopened.budget).toHaveValue('200');
	await expect(reopened.submitButton).toBeEnabled();
});

test('OCRの累計の金額の上限を指定すると、一覧に上限と使用額が出る', async ({
	page,
	managedUser: user
}) => {
	const adminUsers = new AdminUsersPage(page);
	await adminUsers.open();
	const card = adminUsers.userCard(user.username);

	const dialog = await adminUsers.openOcrLimit(user.username);
	await dialog.budgetMode.selectOption('custom');
	await dialog.budget.fill('200');
	await dialog.submitButton.click();
	await expect(dialog.root).toBeHidden();
	await expect(card).toContainText(
		ja.admin_users_ocr_limit_summary.replace('{budget}', '200').replace('{spent}', '0')
	);

	// 無制限にすると、金額の代わりに「無制限」と出る。
	const reopened = await adminUsers.openOcrLimit(user.username);
	await expect(reopened.budget).toHaveValue('200');
	await reopened.budgetMode.selectOption('unlimited');
	await reopened.submitButton.click();
	await expect(reopened.root).toBeHidden();
	await expect(card).toContainText(
		ja.admin_users_ocr_limit_summary_unlimited.replace('{spent}', '0')
	);
});

test('パスワードを再設定すると、新しいパスワードでだけログインできる', async ({
	page,
	managedUser: user
}) => {
	const adminUsers = new AdminUsersPage(page);
	await adminUsers.open();

	const newPassword = uniquePassword();
	const resetDialog = await adminUsers.openResetPassword(user.username);
	await resetDialog.newPassword.fill(newPassword);
	await resetDialog.newPasswordConfirmation.fill(newPassword);
	// Argon2 を回すため、debug ビルドの並列実行では expect の待ち時間を超えうる。応答そのものを待つ。
	const reset = waitForApiResponse(page, 'PUT', `/admin/users/${user.id}/password`);
	await resetDialog.submitButton.click();
	expect((await reset).ok()).toBe(true);
	// 後片付け (→ fixtures.ts) は今のパスワードでログインし直すため、書き換えておく。
	const oldPassword = user.password;
	user.password = newPassword;
	await closeResult(resetDialog.done);

	expect(await loginStatus(user.request, user.username, oldPassword)).toBe(401);
	expect(await loginStatus(user.request, user.username, newPassword)).toBe(200);
});

test('一覧は、ユーザー ID か名前で打つそばから絞り込み、当てはまる人数を出す', async ({
	page,
	managedUser: user
}) => {
	const adminUsers = new AdminUsersPage(page);
	await adminUsers.open();
	await expect(adminUsers.userCard(user.username)).toBeVisible();

	// 大文字・全角で打っても当てはまる。
	await adminUsers.search.fill(user.username.toUpperCase());
	await expect(adminUsers.userCards).toHaveCount(1);
	await expect(adminUsers.userCard(user.username)).toBeVisible();
	await expect(adminUsers.filteredCount(1)).toBeVisible();

	await adminUsers.search.fill(uniqueId('no-such-user'));
	await expect(adminUsers.userCards).toHaveCount(0);
	await expect(adminUsers.noMatch).toBeVisible();

	await adminUsers.search.clear();
	await expect(adminUsers.userCard(user.username)).toBeVisible();
	await expect(adminUsers.totalCount).toBeVisible();
});

test('ユーザーの閲覧ページで、アカウント情報と記録を見られる', async ({
	page,
	managedUser: user
}) => {
	// 閲覧ページの初期表示は、対象ユーザーのタイムゾーンでの今週。
	const today = todayIn(await fetchTimeZone(user.request));
	await createRecordByApi(user.request, {
		localMeasuredAt: `${today}T07:00`,
		systolic: 124,
		diastolic: 82,
		memo: '起床後'
	});

	const adminUsers = new AdminUsersPage(page);
	await adminUsers.open();
	const detail = await adminUsers.openDetail(user.username, user.id);
	await expect(detail.heading(user.username)).toBeVisible();
	await expect(detail.auditNotice).toBeVisible();

	await expect(detail.accountRow(ja.admin_user_detail_status_label)).toContainText(
		ja.admin_users_status_active
	);
	await expect(detail.accountRow(ja.admin_user_detail_login_method_label)).toContainText(
		ja.admin_user_detail_login_method_password
	);

	// 読み取り専用のため、記録カードに編集ボタンは出ない。
	const { records } = detail;
	await expect(records.getByText('起床後', { exact: true })).toBeVisible();
	await expect(detail.stats.averageCard(ja.bp_stats_morning_average_label)).toContainText(
		/124\s*\/\s*82/
	);
	await expect(detail.recordEditButtons).toHaveCount(0);
	await expect(detail.recordDeleteButtons).toHaveCount(0);
});

test.describe('一般ユーザー', () => {
	test.use({ storageState: NO_LOGIN });

	test('管理画面の入口が出ず、URL を直接開いてもメイン画面へ戻される', async ({
		page,
		throwawayUser: user
	}) => {
		const home = new HomePage(page);
		await home.open();
		await home.header.userMenuButton.click();
		await expect(home.header.menuItem(ja.settings_title)).toBeVisible();
		await expect(home.header.menuItem(ja.admin_users_menu_link)).toHaveCount(0);

		for (const path of ['/admin/users', `/admin/users/${user.id}`]) {
			await page.goto(path);
			await page.waitForURL('/');
			await expect(home.heading).toBeVisible();
		}
		expect((await page.request.get('/api/v1/admin/users')).status()).toBe(403);
	});
});
