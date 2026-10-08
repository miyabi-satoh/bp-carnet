import { beforeEach, describe, expect, it, vi } from 'vitest';
import { page, userEvent } from 'vitest/browser';
import { render } from 'vitest-browser-svelte';
import type { ApiResult } from '$lib/api/errors';
import * as m from '$lib/paraglide/messages.js';

vi.mock('$app/state', () => ({ page: { data: { user: { timezone: 'Asia/Tokyo' } } } }));
vi.mock('$app/navigation', () => ({
	goto: vi.fn(),
	invalidateAll: vi.fn(() => Promise.resolve())
}));
vi.mock('$lib/auth', async (importOriginal) => ({
	...(await importOriginal<typeof import('$lib/auth')>()),
	unlinkIdentity: vi.fn()
}));
import { invalidateAll } from '$app/navigation';
import { unlinkIdentity } from '$lib/auth';
import Harness from './unlink-identity-dialog.test-harness.svelte';

beforeEach(() => {
	vi.mocked(unlinkIdentity).mockReset();
	vi.mocked(invalidateAll).mockClear();
});

describe('UnlinkIdentityDialog', () => {
	it('LINE では LINE 側も取り消すことを伝え、解除すると読み直して閉じる', async () => {
		vi.mocked(unlinkIdentity).mockResolvedValue({ ok: true });
		const screen = await render(Harness, { provider: 'line' });

		await screen.getByRole('button', { name: 'open' }).click();
		await expect
			.element(page.getByText(m.unlink_identity_dialog_title({ provider: 'LINE' })))
			.toBeVisible();
		await expect.element(page.getByText(m.unlink_identity_dialog_line_note())).toBeVisible();

		await page.getByRole('button', { name: m.unlink_identity_dialog_submit_button() }).click();

		await vi.waitFor(() => expect(unlinkIdentity).toHaveBeenCalledWith('line'));
		await vi.waitFor(() => expect(invalidateAll).toHaveBeenCalledTimes(1));
		await expect.element(page.getByRole('dialog')).not.toBeInTheDocument();
	});

	it('Google では LINE 側の取り消しの注記を出さない', async () => {
		const screen = await render(Harness, { provider: 'google' });

		await screen.getByRole('button', { name: 'open' }).click();

		await expect
			.element(page.getByText(m.unlink_identity_dialog_title({ provider: 'Google' })))
			.toBeVisible();
		await expect
			.element(page.getByText(m.unlink_identity_dialog_line_note()))
			.not.toBeInTheDocument();
	});

	it('Apple では Apple 側も取り消すことを伝える', async () => {
		const screen = await render(Harness, { provider: 'apple' });

		await screen.getByRole('button', { name: 'open' }).click();

		await expect
			.element(page.getByText(m.unlink_identity_dialog_title({ provider: 'Apple' })))
			.toBeVisible();
		await expect.element(page.getByText(m.unlink_identity_dialog_apple_note())).toBeVisible();
		await expect
			.element(page.getByText(m.unlink_identity_dialog_line_note()))
			.not.toBeInTheDocument();
	});

	it('解除できなかったら、ダイアログは開いたままエラーを出し、読み直さない', async () => {
		vi.mocked(unlinkIdentity).mockResolvedValue({
			ok: false,
			message: '解除できません'
		} as ApiResult);
		const screen = await render(Harness, { provider: 'google' });

		await screen.getByRole('button', { name: 'open' }).click();
		await page.getByRole('button', { name: m.unlink_identity_dialog_submit_button() }).click();

		await expect.element(page.getByText(m.unlink_identity_dialog_error_title())).toBeVisible();
		expect(invalidateAll).not.toHaveBeenCalled();
	});

	it('解除中はキャンセルできず、Esc でも閉じない', async () => {
		let finish: (result: ApiResult) => void = () => {};
		vi.mocked(unlinkIdentity).mockImplementation(
			() => new Promise((resolve) => (finish = resolve))
		);
		const screen = await render(Harness, { provider: 'google' });

		await screen.getByRole('button', { name: 'open' }).click();
		await page.getByRole('button', { name: m.unlink_identity_dialog_submit_button() }).click();
		await vi.waitFor(() => expect(unlinkIdentity).toHaveBeenCalledTimes(1));

		await expect
			.element(page.getByRole('button', { name: m.common_cancel_button() }))
			.toBeDisabled();
		await userEvent.keyboard('{Escape}');
		await expect.element(page.getByRole('dialog')).toBeInTheDocument();

		finish({ ok: true });
		await expect.element(page.getByRole('dialog')).not.toBeInTheDocument();
	});
});
