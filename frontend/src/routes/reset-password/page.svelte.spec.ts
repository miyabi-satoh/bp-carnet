import { beforeEach, describe, expect, it, vi } from 'vitest';
import { render } from 'vitest-browser-svelte';
import * as m from '$lib/paraglide/messages.js';
import { requiredLabel } from '$lib/required-label';

vi.mock('$app/navigation', () => ({ goto: vi.fn(), invalidateAll: vi.fn() }));
vi.mock('$app/paths', () => ({ resolve: (path: string) => path }));
// テストごとに `page.url` を差し替える (モックのモジュールは、テストの import と同じもの)。
vi.mock('$app/state', () => ({
	page: { url: new URL('http://localhost:3000/reset-password?token=reset-token'), data: {} }
}));
vi.mock('$lib/auth', async (importOriginal) => ({
	...(await importOriginal<typeof import('$lib/auth')>()),
	completePasswordReset: vi.fn(),
	isEmailLinkUsable: vi.fn()
}));
vi.mock('$lib/password', async (importOriginal) => ({
	...(await importOriginal<typeof import('$lib/password')>()),
	fetchPasswordPolicy: vi.fn()
}));
import { completePasswordReset, isEmailLinkUsable } from '$lib/auth';
import { page } from '$app/state';
import { fetchPasswordPolicy } from '$lib/password';
import ResetPasswordPage from './+page.svelte';

beforeEach(() => {
	vi.mocked(completePasswordReset).mockReset();
	vi.mocked(completePasswordReset).mockResolvedValue({ ok: true });
	vi.mocked(isEmailLinkUsable).mockReset();
	vi.mocked(isEmailLinkUsable).mockResolvedValue(true);
	vi.mocked(fetchPasswordPolicy).mockResolvedValue({
		ok: true,
		data: { minLength: 8, requiredClasses: [] }
	});
});

async function setNewPassword(screen: Awaited<ReturnType<typeof render>>) {
	await screen
		.getByLabelText(requiredLabel(m.common_new_password_label()), { exact: true })
		.fill('12345678');
	await screen
		.getByLabelText(requiredLabel(m.common_new_password_confirm_label()))
		.fill('12345678');
	await screen.getByRole('button', { name: m.reset_password_submit_button() }).click();
	await expect.element(screen.getByText(m.reset_password_done_title())).toBeVisible();
}

describe('メールのリンクからの再設定', () => {
	it('再設定できたら、新しいパスワードでログインするよう案内し、ログイン画面へのリンクを出す', async () => {
		page.url = new URL('http://localhost:3000/reset-password?token=reset-token') as typeof page.url;
		const screen = await render(ResetPasswordPage);
		await setNewPassword(screen);

		await expect.element(screen.getByText(m.reset_password_done_description())).toBeVisible();
		await expect
			.element(screen.getByRole('link', { name: m.common_back_to_login_link() }))
			.toBeVisible();
	});

	it('アプリから申し込んだリンク (app=1) では、アプリに戻るよう案内し、ログイン画面へのリンクは出さない', async () => {
		page.url = new URL(
			'http://localhost:3000/reset-password?token=reset-token&app=1'
		) as typeof page.url;
		const screen = await render(ResetPasswordPage);
		await setNewPassword(screen);

		await expect.element(screen.getByText(m.email_link_return_to_app_description())).toBeVisible();
		await expect
			.element(screen.getByRole('link', { name: m.common_back_to_login_link() }))
			.not.toBeInTheDocument();
	});

	it('期限切れ・使用済みのリンクを開いたら、フォームを出さず再設定の申し込みへ導く', async () => {
		page.url = new URL('http://localhost:3000/reset-password?token=reset-token') as typeof page.url;
		vi.mocked(isEmailLinkUsable).mockResolvedValue(false);
		const screen = await render(ResetPasswordPage);

		await expect.element(screen.getByText(m.email_link_expired_title())).toBeVisible();
		await expect
			.element(screen.getByRole('link', { name: m.reset_password_expired_retry_button() }))
			.toHaveAttribute('href', '/reset-password');
		await expect
			.element(screen.getByRole('button', { name: m.reset_password_submit_button() }))
			.not.toBeInTheDocument();
		expect(isEmailLinkUsable).toHaveBeenCalledWith('password-reset', 'reset-token');
	});
});
