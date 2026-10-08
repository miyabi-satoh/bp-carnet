// メールを送る機能 (サインアップ・パスワードの再設定、docs/authentication.md) を、届いたメールのリンクまで通して確かめる。
// メールは本物の SMTP で受け取る (→ scripts/mail-sink.ts)。読めるのは `just e2e-local` だけ。
// 同じ接続元からメールを送らせる要求は10分に5回まで (`AttemptRateLimiter::for_mail_requests`) なので、
// ここで送らせる回数はそれに収める。有効期限・出し直し・レートリミットは backend の結合テスト (tests/api.rs) で確かめる。
import { createUserByApi, loginStatus } from './api-helpers';
import { test, expect, NO_LOGIN } from './fixtures';
import { uniqueId } from './helpers';
import {
	linkIn,
	MAIL_DIR,
	MAIL_UNAVAILABLE_REASON,
	waitForMailTo,
	waitForMailWithSubject
} from './mail-helpers';
import { HomePage } from './pages/home-page';
import { LoginPage } from './pages/login-page';
import { SignupPage } from './pages/signup-page';
import { VerifyEmailPage } from './pages/verify-email-page';
import { uniquePassword } from './user-helpers';

test.use({ storageState: NO_LOGIN });
test.skip(MAIL_DIR === undefined, MAIL_UNAVAILABLE_REASON);

function uniqueEmail(): string {
	return `${uniqueId('e2e-mail')}@example.com`;
}

test('サインアップすると確認メールが届き、リンクからパスワードを設定するとログインした状態になる', async ({
	page,
	adminUserCleanup,
	newLoggedOutRequest
}) => {
	const email = uniqueEmail();
	const password = uniquePassword();
	adminUserCleanup.add(email, password);

	const login = new LoginPage(page);
	await login.open();
	const signup = await login.openSignup();
	await signup.email.fill(email);
	await signup.submitButton.click();
	await expect(signup.sentTitle).toBeVisible();

	const mail = await waitForMailTo(email);
	expect(mail.subject).toBe('【BP Carnet】アカウント作成の確認');
	const link = linkIn(mail, '/verify-email');

	await page.goto(link);
	const verify = new VerifyEmailPage(page);
	await verify.setPassword(password);
	await page.waitForURL('/');
	await expect(new HomePage(page).heading).toBeVisible();

	const request = await newLoggedOutRequest();
	try {
		expect(await loginStatus(request, email, password)).toBe(200);
	} finally {
		await request.dispose();
	}

	// リンクは1回限り。使った後に開くと、フォームを出さずに申し込み直す先へ導く。
	await page.goto(link);
	await expect(verify.linkExpiredTitle).toBeVisible();
	await expect(verify.password).toHaveCount(0);
	await expect(verify.retryButton).toHaveAttribute('href', '/signup');
});

test('登録済みのメールアドレスでサインアップすると、確認のリンクではなくログインへの案内が届く', async ({
	page,
	adminRequest,
	adminUserCleanup
}) => {
	const email = uniqueEmail();
	const password = uniquePassword();
	adminUserCleanup.add(email, password);
	await createUserByApi(adminRequest, email, password);

	const signup = new SignupPage(page);
	await signup.open();
	await signup.email.fill(email);
	await signup.submitButton.click();
	// 登録済みかどうかを画面では明かさない。
	await expect(signup.sentTitle).toBeVisible();

	const mail = await waitForMailTo(email);
	expect(mail.subject).toBe('【BP Carnet】アカウント作成のお申し込みについて');
	linkIn(mail, '/login');
	expect(mail.text).not.toContain('/verify-email');
});

test('パスワードの再設定を申し込むとメールが届き、リンクから新しいパスワードを設定するとそれでログインでき、変わったことも知らされる', async ({
	page,
	adminRequest,
	adminUserCleanup,
	newLoggedOutRequest
}) => {
	const email = uniqueEmail();
	const oldPassword = uniquePassword();
	const newPassword = uniquePassword();
	adminUserCleanup.add(email, newPassword);
	await createUserByApi(adminRequest, email, oldPassword);

	const login = new LoginPage(page);
	await login.open();
	const resetPassword = await login.openForgotPassword();
	await resetPassword.email.fill(email);
	await resetPassword.submitButton.click();
	await expect(resetPassword.sentTitle).toBeVisible();
	// 打ち間違いに気づけるよう、送り先を見せる。
	await expect(page.getByText(email, { exact: true })).toBeVisible();

	const mail = await waitForMailTo(email);
	expect(mail.subject).toBe('【BP Carnet】パスワードの再設定');
	const link = linkIn(mail, '/reset-password');

	await page.goto(link);
	await resetPassword.setNewPassword(newPassword);
	await expect(resetPassword.doneTitle).toBeVisible();

	// 勝手に変えられたときに気づけるよう、変わったことを知らせる。
	const notice = await waitForMailWithSubject(email, '【BP Carnet】パスワードが変更されました');
	linkIn(notice, '/reset-password');

	const request = await newLoggedOutRequest();
	try {
		expect(await loginStatus(request, email, oldPassword)).toBe(401);
		expect(await loginStatus(request, email, newPassword)).toBe(200);
	} finally {
		await request.dispose();
	}

	// リンクは1回限り。使った後に開くと、フォームを出さずに申し込み直す先へ導く。
	await page.goto(link);
	await expect(resetPassword.linkExpiredTitle).toBeVisible();
	await expect(resetPassword.newPassword).toHaveCount(0);
	await resetPassword.retryButton.click();
	await expect(resetPassword.email).toBeVisible();
});
