// 全角の英数字を半角に直す欄。部品を付け忘れていないかを、実際の画面の欄で確かめる。
// 変換中 (IME の未確定) の扱いは部品のテスト (src/lib/half-width.svelte.spec.ts) で見る。
import type { Locator } from '@playwright/test';
import { test, expect, NO_LOGIN } from './fixtures';
import { AdminUsersPage } from './pages/admin-users-page';
import { HomePage } from './pages/home-page';
import { LoginPage } from './pages/login-page';
import { ResetPasswordPage } from './pages/reset-password-page';

/** 欄に文字を確定した形で入れる (IME で確定したときと同じく、変換中でない `input` が届く)。 */
async function insertText(field: Locator, text: string) {
	await field.click();
	await field.page().keyboard.insertText(text);
}

test('記録フォームの数字の欄は半角に直し、メモは直さない', async ({ page }) => {
	const home = new HomePage(page);
	await home.open();
	const { fields: sheetFields } = await home.openAddSheet();

	const fields: [Locator, string, string][] = [
		[sheetFields.systolic, '１２０', '120'],
		[sheetFields.diastolic, '８０', '80'],
		[sheetFields.pulse, '７０', '70'],
		[sheetFields.memo, '朝の薬１２０', '朝の薬１２０']
	];
	for (const [field, typed, expected] of fields) {
		await insertText(field, typed);
		await expect(field).toHaveValue(expected);
	}
});

test('ユーザーの追加のユーザーIDは半角に直し、パスワードは直さない', async ({ page }) => {
	const adminUsers = new AdminUsersPage(page);
	await adminUsers.open();
	const { username, password } = await adminUsers.openAddUser();

	await insertText(username, 'ｔａｒｏ＿０１');
	await expect(username).toHaveValue('taro_01');
	await insertText(password, 'ｐａｓｓ');
	await expect(password).toHaveValue('ｐａｓｓ');
});

test('OCRの累計の金額は半角に直す', async ({ page, managedUser: user }) => {
	const adminUsers = new AdminUsersPage(page);
	await adminUsers.open();
	const { budgetMode, budget } = await adminUsers.openOcrLimit(user.username);
	await budgetMode.selectOption('custom');

	await insertText(budget, '８０');
	await expect(budget).toHaveValue('80');
});

test.describe('未ログイン', () => {
	test.use({ storageState: NO_LOGIN });

	test('ログインのユーザーIDは半角に直し、パスワードは直さない', async ({ page }) => {
		const login = new LoginPage(page);
		await login.open();
		const { username, password } = login;
		await insertText(username, 'ａｄｍｉｎ');
		await expect(username).toHaveValue('admin');
		await insertText(password, 'ｐａｓｓ');
		await expect(password).toHaveValue('ｐａｓｓ');
	});

	test('パスワードの再設定のメールアドレスは半角に直す', async ({ page }) => {
		const resetPassword = new ResetPasswordPage(page);
		await resetPassword.open();

		const { email } = resetPassword;
		await insertText(email, 'ｔａｒｏ＠ｅｘａｍｐｌｅ．ｃｏｍ');
		await expect(email).toHaveValue('taro@example.com');
	});
});
