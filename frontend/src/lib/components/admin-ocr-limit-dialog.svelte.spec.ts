import { beforeEach, describe, expect, it, vi } from 'vitest';
import { page, userEvent } from 'vitest/browser';
import { render } from 'vitest-browser-svelte';
import type { AdminUser } from '$lib/admin';
import type { ApiResult } from '$lib/api/errors';
import * as m from '$lib/paraglide/messages.js';
import { requiredLabel } from '$lib/required-label';

vi.mock('$lib/admin', async (importOriginal) => ({
	...(await importOriginal<typeof import('$lib/admin')>()),
	setUserOcrLimit: vi.fn()
}));
import { setUserOcrLimit } from '$lib/admin';
import { closeIcon } from './dialog.test-helpers';
import Harness from './admin-ocr-limit-dialog.test-harness.svelte';

const USER: AdminUser = {
	id: 2,
	username: 'hiroshi',
	role: 'user',
	frozen: false,
	passwordUsable: true,
	ocrBudgetYen: null,
	ocrSpentYen: 2,
	ocrPaidRemainingYen: 0,
	lastSeenAt: '2026-09-20T00:00:00.000Z'
};

function pendingSave() {
	let finish: (result: ApiResult) => void = () => {};
	vi.mocked(setUserOcrLimit).mockImplementation(() => new Promise((resolve) => (finish = resolve)));
	return (result: ApiResult) => finish(result);
}

beforeEach(() => {
	vi.mocked(setUserOcrLimit).mockReset();
});

describe('AdminOcrLimitDialog', () => {
	// bits-ui の Dialog.Content は props が変わると中身を作り直す。送信の開始・終了で作り直されると、
	// 閉じる途中の中身が置き去りになり、覆いが画面の操作を塞ぐ。
	it('送信中は中身を作り直さず、× は無効で Esc でも閉じない。保存できたら閉じ、開き直せる', async () => {
		const finish = pendingSave();
		const screen = await render(Harness, { user: USER });
		await screen.getByRole('button', { name: 'open' }).click();
		await page.getByLabelText(m.admin_users_ocr_budget_mode_label()).selectOptions('custom');
		const yen = page.getByLabelText(requiredLabel(m.admin_users_ocr_budget_label()));
		await yen.fill('200');
		const dialog = page.getByRole('dialog').element();
		await page.getByRole('button', { name: m.admin_users_ocr_limit_submit_button() }).click();
		await vi.waitFor(() => expect(setUserOcrLimit).toHaveBeenCalledWith(USER.id, 200));
		await vi.waitFor(() => expect(closeIcon().disabled).toBe(true));
		expect(page.getByRole('dialog').element()).toBe(dialog);
		await userEvent.keyboard('{Escape}');
		await expect.element(page.getByRole('dialog')).toBeInTheDocument();
		finish({ ok: true });
		await expect.element(screen.getByTestId('saved-count')).toHaveTextContent('1');
		await expect.element(page.getByRole('dialog')).not.toBeInTheDocument();

		await screen.getByRole('button', { name: 'open' }).click();
		await expect
			.element(page.getByRole('button', { name: m.admin_users_ocr_limit_submit_button() }))
			.toBeEnabled();
		expect(closeIcon().disabled).toBe(false);
	});

	it('累計の金額は、既定値・無制限・金額の指定から選べる', async () => {
		pendingSave();
		const screen = await render(Harness, { user: USER });
		await screen.getByRole('button', { name: 'open' }).click();
		const mode = page.getByLabelText(m.admin_users_ocr_budget_mode_label());
		await expect.element(mode).toHaveValue('default');
		await expect
			.element(
				page.getByRole('option', { name: m.admin_users_ocr_budget_mode_default({ yen: 80 }) })
			)
			.toBeInTheDocument();
		await mode.selectOptions('custom');
		await page.getByLabelText(requiredLabel(m.admin_users_ocr_budget_label())).fill('200');
		await page.getByRole('button', { name: m.admin_users_ocr_limit_submit_button() }).click();
		await vi.waitFor(() => expect(setUserOcrLimit).toHaveBeenCalledWith(USER.id, 200));
	});

	it('累計の金額が値域外なら送らずにエラーを出す', async () => {
		pendingSave();
		const screen = await render(Harness, { user: USER });
		await screen.getByRole('button', { name: 'open' }).click();
		await page.getByLabelText(m.admin_users_ocr_budget_mode_label()).selectOptions('custom');
		await page.getByLabelText(requiredLabel(m.admin_users_ocr_budget_label())).fill('100001');
		await page.getByRole('button', { name: m.admin_users_ocr_limit_submit_button() }).click();
		await expect
			.element(page.getByText(m.admin_users_ocr_budget_error({ min: 0, max: 100000 })))
			.toBeVisible();
		expect(setUserOcrLimit).not.toHaveBeenCalled();
	});
});
