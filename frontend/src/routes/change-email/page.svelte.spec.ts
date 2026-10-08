import { beforeEach, describe, expect, it, vi } from 'vitest';
import { render } from 'vitest-browser-svelte';
import * as m from '$lib/paraglide/messages.js';

vi.mock('$app/navigation', () => ({ goto: vi.fn() }));
vi.mock('$app/paths', () => ({ resolve: (path: string) => path }));
// テストごとに `page.url` を差し替える (モックのモジュールは、テストの import と同じもの)。
vi.mock('$app/state', () => ({
	page: { url: new URL('http://localhost:3000/change-email?token=change-token'), data: {} }
}));
vi.mock('$lib/auth', async (importOriginal) => ({
	...(await importOriginal<typeof import('$lib/auth')>()),
	completeEmailChange: vi.fn(),
	emailChangeTarget: vi.fn()
}));
import { completeEmailChange, emailChangeTarget } from '$lib/auth';
import { page } from '$app/state';
import ChangeEmailPage from './+page.svelte';

beforeEach(() => {
	page.url = new URL('http://localhost:3000/change-email?token=change-token') as typeof page.url;
	vi.mocked(completeEmailChange).mockReset();
	vi.mocked(completeEmailChange).mockResolvedValue({ ok: true });
	vi.mocked(emailChangeTarget).mockReset();
	vi.mocked(emailChangeTarget).mockResolvedValue({
		ok: true,
		data: { newEmail: 'new@example.com' }
	});
});

describe('メールアドレスの変更の確定', () => {
	it('開いただけでは変えず、変更先を見せて「変更する」で確定する', async () => {
		const screen = await render(ChangeEmailPage);

		await expect.element(screen.getByText('new@example.com')).toBeVisible();
		expect(completeEmailChange).not.toHaveBeenCalled();

		await screen.getByRole('button', { name: m.change_email_submit_button() }).click();

		await vi.waitFor(() => expect(completeEmailChange).toHaveBeenCalledWith('change-token'));
		await expect.element(screen.getByText(m.change_email_done_title())).toBeVisible();
		await expect.element(screen.getByText(m.change_email_done_description())).toBeVisible();
	});

	it('アプリから申し込んだリンク (app=1) では、アプリに戻るよう案内する', async () => {
		page.url = new URL(
			'http://localhost:3000/change-email?token=change-token&app=1'
		) as typeof page.url;
		const screen = await render(ChangeEmailPage);

		await screen.getByRole('button', { name: m.change_email_submit_button() }).click();

		await expect.element(screen.getByText(m.change_email_done_app_description())).toBeVisible();
		await expect
			.element(screen.getByRole('link', { name: m.change_email_home_button() }))
			.not.toBeInTheDocument();
	});

	it('使えないリンクなら、申し込み直す先へ導く', async () => {
		vi.mocked(emailChangeTarget).mockResolvedValue({
			ok: false,
			message: m.error_invalid_email_token(),
			code: 'invalid_email_token'
		});
		const screen = await render(ChangeEmailPage);

		await expect
			.element(screen.getByRole('link', { name: m.change_email_expired_retry_button() }))
			.toHaveAttribute('href', '/settings/change-email');
	});

	it('確定の前に別のアカウントがアドレスを使ったら、変えられないことを知らせる', async () => {
		vi.mocked(completeEmailChange).mockResolvedValue({
			ok: false,
			message: m.error_username_taken(),
			code: 'username_taken'
		});
		const screen = await render(ChangeEmailPage);

		await screen.getByRole('button', { name: m.change_email_submit_button() }).click();

		await expect.element(screen.getByText(m.change_email_taken_description())).toBeVisible();
	});
});
