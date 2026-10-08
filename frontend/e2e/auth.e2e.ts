// ログイン・ログアウトと、未ログイン時の誘導。
// e2eプロジェクトの既定storageStateは管理者ログイン済みのため、このファイルでは
// 空のstorageStateに上書きして未ログイン状態から始める。
// ログアウトは今のセッションだけを破棄するので、他のテストが使い回すログイン状態は失効しない。
import { ADMIN_USERNAME, ADMIN_PASSWORD } from './auth';
import { test, expect, NO_LOGIN } from './fixtures';
import { HomePage } from './pages/home-page';
import { LoginPage } from './pages/login-page';

test.use({ storageState: NO_LOGIN });

test('ログインするとメイン画面が出て、ログアウトするとログイン画面に戻る', async ({ page }) => {
	const login = new LoginPage(page);
	const home = await login.logIn(ADMIN_USERNAME, ADMIN_PASSWORD);
	await expect(home.heading).toBeVisible();

	// ログアウトは確認を挟む。
	const logout = await home.header.openLogout();
	await logout.confirm();
	await expect(page.getByRole('alertdialog')).toHaveCount(0);
	await expect(login.submitButton).toBeVisible();
});

test('ログアウトに失敗したら、確認のダイアログを開いたまま知らせ、その場でやり直せる', async ({
	page
}) => {
	const login = new LoginPage(page);
	const home = await login.logIn(ADMIN_USERNAME, ADMIN_PASSWORD);
	await expect(home.heading).toBeVisible();
	await page.route('**/api/v1/auth/logout', (route) =>
		route.fulfill({ status: 500, json: { error: { message: 'x' } } })
	);

	// API がエラーを返したときは、ダイアログを開いたままにする。
	const logout = await home.header.openLogout();
	await logout.confirmButton.click();
	await logout.closeError();
	await expect(logout.errorDialog).toHaveCount(0);
	await expect(logout.root).toBeVisible();

	await page.unroute('**/api/v1/auth/logout');
	await logout.confirm();
	await expect(page.getByRole('alertdialog')).toHaveCount(0);
});

test('ログインした後は、戻る操作でログイン画面に戻らず、ログイン画面を開いてもホーム画面へ送られる', async ({
	page
}) => {
	// ログイン画面の前に別の画面を開いておき、戻る操作でどこへ戻るかを見る。
	await page.goto('/help');
	await new LoginPage(page).logIn(ADMIN_USERNAME, ADMIN_PASSWORD);

	await page.goBack();
	await page.waitForURL('/help');

	// 古い履歴やブックマークからログイン画面を開いた場合。
	await page.goto('/login');
	await page.waitForURL('/');
	await expect(new HomePage(page).heading).toBeVisible();
});

test('ログインしていないと、保護された画面からログイン画面へ戻される', async ({ page }) => {
	await page.goto('/settings');
	await page.waitForURL('/login');
	await expect(new LoginPage(page).submitButton).toBeVisible();
});

test('パスワードが違うとダイアログで知らせ、「ログインできないとき」の使い方へ移れる', async ({
	page
}) => {
	const login = new LoginPage(page);
	await login.open();
	await login.username.fill(ADMIN_USERNAME);
	await login.password.fill(`${ADMIN_PASSWORD}-wrong`);
	await login.submitButton.click();
	await login.errorHelpLink.click();

	await expect(page).toHaveURL('/help/trouble-login');
});
