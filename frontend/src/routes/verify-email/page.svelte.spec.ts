import { beforeEach, describe, expect, it, vi } from 'vitest';
import { render } from 'vitest-browser-svelte';
import * as m from '$lib/paraglide/messages.js';
import { requiredLabel } from '$lib/required-label';

vi.mock('$app/navigation', () => ({ goto: vi.fn() }));
vi.mock('$app/paths', () => ({ resolve: (path: string) => path }));
// テストごとに `page.url` を差し替える (モックのモジュールは、テストの import と同じもの)。
vi.mock('$app/state', () => ({
	page: { url: new URL('http://localhost:3000/verify-email?token=signup-token'), data: {} }
}));
vi.mock('$lib/auth', async (importOriginal) => ({
	...(await importOriginal<typeof import('$lib/auth')>()),
	completeSignup: vi.fn(),
	isEmailLinkUsable: vi.fn()
}));
vi.mock('$lib/password', async (importOriginal) => ({
	...(await importOriginal<typeof import('$lib/password')>()),
	fetchPasswordPolicy: vi.fn()
}));
import { goto } from '$app/navigation';
import { completeSignup, isEmailLinkUsable } from '$lib/auth';
import { page } from '$app/state';
import { fetchPasswordPolicy } from '$lib/password';
import VerifyEmailPage from './+page.svelte';

beforeEach(() => {
	page.url = new URL('http://localhost:3000/verify-email?token=signup-token') as typeof page.url;
	vi.mocked(goto).mockReset();
	vi.mocked(completeSignup).mockReset();
	vi.mocked(completeSignup).mockResolvedValue({ ok: true });
	vi.mocked(isEmailLinkUsable).mockReset();
	vi.mocked(isEmailLinkUsable).mockResolvedValue(true);
	vi.mocked(fetchPasswordPolicy).mockResolvedValue({
		ok: true,
		data: { minLength: 8, requiredClasses: [] }
	});
});

describe('サインアップの仕上げ', () => {
	it('パスワードを決めたら、この画面を履歴に残さずホームへ進む', async () => {
		const screen = await render(VerifyEmailPage);

		await screen
			.getByLabelText(requiredLabel(m.common_password_label()), { exact: true })
			.fill('12345678');
		await screen.getByLabelText(requiredLabel(m.common_password_confirm_label())).fill('12345678');
		await screen.getByRole('button', { name: m.verify_email_submit_button() }).click();

		await vi.waitFor(() => expect(completeSignup).toHaveBeenCalledWith('signup-token', '12345678'));
		await vi.waitFor(() => expect(goto).toHaveBeenCalledWith('/', { replaceState: true }));
	});

	it('アプリから申し込んだリンク (app=1) では、ホームへ進まずアプリに戻るよう案内する', async () => {
		page.url = new URL(
			'http://localhost:3000/verify-email?token=signup-token&app=1'
		) as typeof page.url;
		const screen = await render(VerifyEmailPage);

		await screen
			.getByLabelText(requiredLabel(m.common_password_label()), { exact: true })
			.fill('12345678');
		await screen.getByLabelText(requiredLabel(m.common_password_confirm_label())).fill('12345678');
		await screen.getByRole('button', { name: m.verify_email_submit_button() }).click();

		await expect.element(screen.getByText(m.verify_email_done_title())).toBeVisible();
		await expect.element(screen.getByText(m.email_link_return_to_app_description())).toBeVisible();
		expect(completeSignup).toHaveBeenCalledWith('signup-token', '12345678');
		expect(goto).not.toHaveBeenCalled();
		await expect
			.element(screen.getByRole('link', { name: m.common_back_to_login_link() }))
			.not.toBeInTheDocument();
	});

	it('期限切れ・使用済みのリンクを開いたら、フォームを出さず確認メールを送り直す先へ導く', async () => {
		vi.mocked(isEmailLinkUsable).mockResolvedValue(false);
		const screen = await render(VerifyEmailPage);

		await expect.element(screen.getByText(m.email_link_expired_title())).toBeVisible();
		await expect
			.element(screen.getByRole('link', { name: m.verify_email_expired_retry_button() }))
			.toHaveAttribute('href', '/signup');
		await expect
			.element(screen.getByRole('button', { name: m.verify_email_submit_button() }))
			.not.toBeInTheDocument();
		expect(isEmailLinkUsable).toHaveBeenCalledWith('signup', 'signup-token');
	});

	it('入力している間にリンクが使えなくなったら、送った後に同じ案内へ差し替える', async () => {
		vi.mocked(completeSignup).mockResolvedValue({
			ok: false,
			message: m.error_invalid_email_token(),
			code: 'invalid_email_token'
		});
		const screen = await render(VerifyEmailPage);

		await screen
			.getByLabelText(requiredLabel(m.common_password_label()), { exact: true })
			.fill('12345678');
		await screen.getByLabelText(requiredLabel(m.common_password_confirm_label())).fill('12345678');
		await screen.getByRole('button', { name: m.verify_email_submit_button() }).click();

		await expect.element(screen.getByText(m.email_link_expired_title())).toBeVisible();
		expect(goto).not.toHaveBeenCalled();
	});
});
