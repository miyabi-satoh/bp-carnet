// ダイアログの種類ごとの閉じ方。× を出すのはフォームだけで、確認のダイアログはキャンセル・Esc で閉じる。
// 共有の管理者で流す。
import { createRecordByApi, fetchTimeZone } from './api-helpers';
import { todayIn } from './date-helpers';
import { test, expect } from './fixtures';
import { uniqueId } from './helpers';
import { AdminUsersPage } from './pages/admin-users-page';
import { HomePage } from './pages/home-page';
import { SettingsPage } from './pages/settings-page';

test('フォームのダイアログは見出しの × で閉じられる', async ({ page }) => {
	const adminUsers = new AdminUsersPage(page);
	await adminUsers.open();
	const dialog = await adminUsers.openAddUser();

	await dialog.closeButton.click();
	await expect(dialog.root).toBeHidden();
});

test('フォームのダイアログは、開き直しても最初の入力欄にフォーカスが入り、そのまま打てる', async ({
	page
}) => {
	const adminUsers = new AdminUsersPage(page);
	await adminUsers.open();

	// 1回目はパスワードの決まりを読み込んでから欄が出るので、2回目 (読み込み済み) で確かめる。
	const dialog = await adminUsers.openAddUser();
	const { username } = dialog;
	await expect(username).toBeVisible();
	await page.keyboard.press('Escape');
	await expect(dialog.root).toBeHidden();

	await adminUsers.openAddUser();
	await expect(username).toBeFocused();
	await page.keyboard.type('typed-user');
	await expect(username).toHaveValue('typed-user');
});

test('戻せない操作の確認は × を出さず、Esc とキャンセルで閉じる', async ({
	page,
	adminRecordCleanup
}) => {
	const memo = uniqueId('e2e-dialog-kind');
	adminRecordCleanup.add(memo);
	await createRecordByApi(page.request, {
		localMeasuredAt: `${todayIn(await fetchTimeZone(page.request))}T00:00`,
		systolic: 120,
		diastolic: 80,
		memo
	});
	const home = new HomePage(page);
	await home.open();
	const confirm = await home.openDelete(memo);

	await expect(confirm.root).toBeVisible();
	await expect(confirm.closeButton).toHaveCount(0);
	await page.keyboard.press('Escape');
	await expect(confirm.root).toBeHidden();

	await home.openDelete(memo);
	await confirm.cancelButton.click();
	await expect(confirm.root).toBeHidden();
	await expect(page.getByText(memo, { exact: true })).toHaveCount(1);
});

test('軽い確認は × を出さず、キャンセルで閉じる', async ({ page }) => {
	const settings = new SettingsPage(page);
	await settings.open();
	const dialog = await settings.openLogout();

	await expect(dialog.root).toBeVisible();
	await expect(dialog.closeButton).toHaveCount(0);
	await dialog.cancelButton.click();
	await expect(dialog.root).toBeHidden();
	await expect(page).toHaveURL('/settings');
});
