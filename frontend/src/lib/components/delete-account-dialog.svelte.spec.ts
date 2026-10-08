import { beforeEach, describe, expect, it, vi } from 'vitest';
import { page, userEvent } from 'vitest/browser';
import { render } from 'vitest-browser-svelte';
import type { ApiResult } from '$lib/api/errors';
import * as m from '$lib/paraglide/messages.js';
import { requiredLabel } from '$lib/required-label';

vi.mock('$app/state', () => ({
	page: { data: { user: { timezone: 'Asia/Tokyo' } } }
}));
vi.mock('$lib/auth', async (importOriginal) => ({
	...(await importOriginal<typeof import('$lib/auth')>()),
	scheduleAccountDeletion: vi.fn()
}));
import { scheduleAccountDeletion } from '$lib/auth';
import Harness from './delete-account-dialog.test-harness.svelte';

beforeEach(() => {
	vi.mocked(scheduleAccountDeletion).mockReset();
});

describe('DeleteAccountDialog', () => {
	// bits-ui の Dialog.Content は props が変わると中身を作り直す。送信の開始・終了で作り直されると、
	// 入力や結果の表示がダイアログごと描き直される。
	it('送信中は中身を作り直さず、キャンセルは無効で Esc でも閉じない。予約できたら同じダイアログに結果を出す', async () => {
		let finish: (result: ApiResult) => void = () => {};
		vi.mocked(scheduleAccountDeletion).mockImplementation(
			() => new Promise((resolve) => (finish = resolve))
		);
		const screen = await render(Harness);

		await screen.getByRole('button', { name: 'open' }).click();
		const confirmation = page.getByLabelText(
			requiredLabel(m.delete_account_dialog_confirm_label())
		);
		await confirmation.fill(m.delete_account_dialog_confirm_word());
		const input = confirmation.element();
		const dialog = page.getByRole('dialog').element();

		await page.getByRole('button', { name: m.delete_account_dialog_submit_button() }).click();
		await vi.waitFor(() => expect(scheduleAccountDeletion).toHaveBeenCalledTimes(1));

		await expect
			.element(page.getByRole('button', { name: m.common_cancel_button() }))
			.toBeDisabled();
		expect(page.getByRole('dialog').element()).toBe(dialog);
		expect(confirmation.element()).toBe(input);
		await userEvent.keyboard('{Escape}');
		await expect.element(page.getByRole('dialog')).toBeInTheDocument();

		finish({ ok: true });
		await expect.element(page.getByText(m.delete_account_dialog_scheduled_title())).toBeVisible();
		expect(page.getByRole('dialog').element()).toBe(dialog);

		await userEvent.keyboard('{Escape}');
		await expect.element(page.getByRole('dialog')).not.toBeInTheDocument();
	});
});
