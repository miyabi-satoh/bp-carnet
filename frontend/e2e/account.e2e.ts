// 本人によるアカウントの削除。ユーザーの状態を変えるため、使い捨てユーザーで流す。
import { findUserByApi } from './api-helpers';
import { userTest as test, expect, NO_LOGIN } from './fixtures';
import { clearLogin, ja } from './helpers';
import { LoginPage } from './pages/login-page';
import { SettingsPage } from './pages/settings-page';

test.use({ storageState: NO_LOGIN });

test('アカウントの削除を予約し、猶予期間中にログインすると取り消される', async ({
	page,
	adminRequest,
	throwawayUser: user
}) => {
	const settings = new SettingsPage(page);
	await settings.open();
	const dialog = await settings.openDeleteAccount();
	await expect(dialog.submitButton).toBeDisabled();
	await dialog.confirmation.fill(ja.delete_account_dialog_confirm_word);
	await expect(dialog.submitButton).toBeEnabled();

	await dialog.submitButton.click();
	await expect(dialog.scheduledResult).toBeVisible();
	expect((await findUserByApi(adminRequest, user.username))?.deletionScheduledAt).toBeTruthy();

	// 閉じた後の設定画面には、ボタンの代わりに予定日と取り消し方が出る。
	await page.keyboard.press('Escape');
	await expect(dialog.scheduledResult).toHaveCount(0);
	await expect(settings.deletionCancelHint).toBeVisible();
	await expect(settings.deleteAccountButton).toHaveCount(0);

	// ログアウトした状態から入り直すと、予約が取り消されたことが知らされる。
	await clearLogin(page);
	const home = await new LoginPage(page).logIn(user.username, user.password);
	await expect(home.deletionCancelledNotice).toBeVisible();
	expect(new URL(page.url()).pathname).toBe('/');
	const after = await findUserByApi(adminRequest, user.username);
	expect(after).toBeDefined();
	expect(after?.deletionScheduledAt ?? null).toBeNull();
});
