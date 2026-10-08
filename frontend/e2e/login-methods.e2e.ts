// 設定画面の「ログイン方法」(連携の解除、docs/authentication.md)。e2e の backend は LINE・Google でログインできない
// ため、連携のある状態は `/auth/me` の応答を差し替えて作る (→ auth-mocks.ts)。解除の判定そのものは
// backend の結合テスト (tests/api.rs) で確かめる。
import { mockLinkedProviders } from './auth-mocks';
import { userTest as test, expect, NO_LOGIN } from './fixtures';
import { ja } from './helpers';
import { SettingsPage } from './pages/settings-page';

test.use({ storageState: NO_LOGIN });

test('連携が無いユーザーには「ログイン方法」の欄を出さない', async ({ page }) => {
	const settings = new SettingsPage(page);
	await settings.open();

	await expect(settings.account).toBeVisible();
	await expect(settings.loginMethods).toHaveCount(0);
});

test('パスワードがあれば連携を解除でき、解除した方法は一覧から消える', async ({ page }) => {
	const { unlinkRequests } = await mockLinkedProviders(page, {
		passwordUsable: true,
		linkedProviders: ['line']
	});
	const settings = new SettingsPage(page);
	await settings.open();

	await expect(settings.lastMethodNotes).toHaveCount(0);
	const dialog = await settings.openUnlink('LINE');
	await expect(dialog.lineNote).toBeVisible();
	await dialog.submitButton.click();

	await expect(dialog.root).toBeHidden();
	expect(unlinkRequests).toEqual(['line']);
	// 最後の連携を外すと、欄ごと消える。押したボタンも消えるので、フォーカスはページの見出しへ移る。
	await expect(settings.loginMethods).toHaveCount(0);
	await expect(settings.heading).toBeFocused();
});

test('取り消しを選んだら、何も解除しない', async ({ page }) => {
	const { unlinkRequests } = await mockLinkedProviders(page, {
		passwordUsable: true,
		linkedProviders: ['google']
	});
	const settings = new SettingsPage(page);
	await settings.open();

	const dialog = await settings.openUnlink('Google');
	// LINE 側の取り消しの注記は LINE の解除にだけ出る。
	await expect(dialog.lineNote).toHaveCount(0);
	await dialog.cancelButton.click();

	await expect(dialog.root).toBeHidden();
	expect(unlinkRequests).toEqual([]);
	await expect(settings.unlinkButton('Google')).toBeVisible();
});

test('パスワードが無くても連携が2つあれば片方は外せて、残った1つは外せない', async ({ page }) => {
	await mockLinkedProviders(page, { passwordUsable: false, linkedProviders: ['line', 'google'] });
	const settings = new SettingsPage(page);
	await settings.open();

	// LINE が上、Google が下。どちらも外せる。
	await expect(settings.unlinkButton('LINE')).toBeVisible();
	await expect(settings.unlinkButton('Google')).toBeVisible();
	await expect(settings.lastMethodNotes).toHaveCount(0);

	const dialog = await settings.openUnlink('Google');
	await dialog.submitButton.click();
	await expect(dialog.root).toBeHidden();

	// 残った LINE は最後のログイン方法。ボタンを出さず、理由を出す。
	await expect(settings.unlinkButton('LINE')).toHaveCount(0);
	await expect(settings.lastMethodNotes).toBeVisible();
	await expect(settings.loginMethods).toContainText('LINE');
	await expect(settings.loginMethods).not.toContainText('Google');
	// 押したボタンは行ごと消えるので、フォーカスは欄の見出しへ移る。
	await expect(settings.loginMethodsHeading).toBeFocused();
});

test('パスワードも他の連携も無いときは、ボタンを出さずに理由を出す', async ({ page }) => {
	await mockLinkedProviders(page, { passwordUsable: false, linkedProviders: ['google'] });
	const settings = new SettingsPage(page);
	await settings.open();

	await expect(settings.loginMethods).toContainText('Google');
	await expect(settings.lastMethodNotes).toBeVisible();
	await expect(settings.unlinkButton('Google')).toHaveCount(0);
});

test('画面を開いたあとに状態が変わって解除を断られたら、理由を知らせて連携は残す', async ({
	page
}) => {
	await mockLinkedProviders(page, { passwordUsable: true, linkedProviders: ['google'] });
	// 画面を開いている間に、ほかの端末でパスワードが無くなった、などの食い違い。
	await page.route('**/api/v1/account/identities/*', (route) =>
		route.fulfill({
			status: 409,
			json: { error: { code: 'cannot_unlink_last_login_method', message: 'conflict' } }
		})
	);
	const settings = new SettingsPage(page);
	await settings.open();

	const dialog = await settings.openUnlink('Google');
	await dialog.submitButton.click();

	await expect(dialog.errorDialog).toContainText(ja.error_cannot_unlink_last_login_method);
	await expect(settings.unlinkButton('Google')).toBeVisible();
});
