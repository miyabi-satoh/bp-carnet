import { beforeEach, describe, expect, it, vi } from 'vitest';
import { render } from 'vitest-browser-svelte';
import * as m from '$lib/paraglide/messages.js';
import { requiredLabel } from '$lib/required-label';

vi.mock('$app/navigation', () => ({ goto: vi.fn() }));
vi.mock('$app/paths', () => ({ resolve: (path: string) => path }));
vi.mock('$app/state', () => ({
	page: {
		url: new URL('http://localhost:3000/settings/change-email'),
		data: { user: { username: 'kyoko@example.com', passwordUsable: true } }
	}
}));
vi.mock('$lib/auth', async (importOriginal) => ({
	...(await importOriginal<typeof import('$lib/auth')>()),
	requestEmailChange: vi.fn()
}));
import { requestEmailChange } from '$lib/auth';
import { page } from '$app/state';
import ChangeEmailPage from './+page.svelte';

beforeEach(() => {
	page.data = { user: { username: 'kyoko@example.com', passwordUsable: true } } as typeof page.data;
	vi.mocked(requestEmailChange).mockReset();
	vi.mocked(requestEmailChange).mockResolvedValue({ ok: true });
});

describe('メールアドレスの変更の申し込み', () => {
	it('新しいアドレスと今のパスワードを送り、送った先を出す', async () => {
		const screen = await render(ChangeEmailPage);

		await expect.element(screen.getByText('kyoko@example.com')).toBeVisible();
		await screen
			.getByLabelText(requiredLabel(m.settings_change_email_new_label()))
			.fill('new@example.com');
		await screen
			.getByLabelText(requiredLabel(m.settings_change_email_password_label()))
			.fill('password');
		await screen.getByRole('button', { name: m.settings_change_email_submit_button() }).click();

		await vi.waitFor(() =>
			expect(requestEmailChange).toHaveBeenCalledWith('new@example.com', 'password')
		);
		await expect.element(screen.getByText(m.settings_change_email_sent_title())).toBeVisible();
		await expect.element(screen.getByText('new@example.com')).toBeVisible();

		// 送り先を直すと、入れたアドレスを残してフォームに戻る。押したボタンが消えるので、メール欄に移る。
		await screen.getByRole('button', { name: m.email_request_edit_button() }).click();
		const newEmailInput = screen.getByLabelText(requiredLabel(m.settings_change_email_new_label()));
		await expect.element(newEmailInput).toHaveValue('new@example.com');
		await expect.element(newEmailInput).toHaveFocus();
	});

	it('パスワードを持たないアカウントには、パスワードの欄を出さない', async () => {
		page.data = {
			user: { username: 'google@example.com', passwordUsable: false }
		} as typeof page.data;
		const screen = await render(ChangeEmailPage);

		await expect
			.element(screen.getByLabelText(requiredLabel(m.settings_change_email_password_label())))
			.not.toBeInTheDocument();
		await screen
			.getByLabelText(requiredLabel(m.settings_change_email_new_label()))
			.fill('new@example.com');
		await screen.getByRole('button', { name: m.settings_change_email_submit_button() }).click();

		await vi.waitFor(() =>
			expect(requestEmailChange).toHaveBeenCalledWith('new@example.com', null)
		);
	});
});
