import { beforeEach, describe, expect, it, vi } from 'vitest';
import { page } from 'vitest/browser';
import { render } from 'vitest-browser-svelte';
import * as m from '$lib/paraglide/messages.js';

vi.mock('$app/paths', () => ({ resolve: (path: string) => path }));
vi.mock('$lib/external-navigation', () => ({ navigateTo: vi.fn() }));
vi.mock('$lib/payments', () => ({ startOcrTopupCheckout: vi.fn() }));
vi.mock('$lib/app-store-purchase', () => ({ purchaseTopup: vi.fn(), topupDisplayPrice: vi.fn() }));
vi.mock('$lib/auth', () => ({ fetchContactUrl: vi.fn() }));
vi.mock('$lib/tax', () => ({ isTaxIncluded: vi.fn() }));
import { navigateTo } from '$lib/external-navigation';
import { startOcrTopupCheckout } from '$lib/payments';
import { isTaxIncluded } from '$lib/tax';
import Harness from './topup-confirm-dialog.test-harness.svelte';

beforeEach(() => {
	vi.mocked(startOcrTopupCheckout).mockReset();
	vi.mocked(navigateTo).mockReset();
	vi.mocked(isTaxIncluded).mockReturnValue(false);
});

describe('TopupConfirmDialog', () => {
	it('インボイスの登録の日から、消費税なしを税込みに替えて出す', async () => {
		const screen = await render(Harness);

		await screen.getByRole('button', { name: 'open' }).click();
		await expect
			.element(page.getByText(m.topup_confirm_tax_exempt(), { exact: false }))
			.toBeVisible();

		await page.getByRole('button', { name: m.common_cancel_button() }).click();
		vi.mocked(isTaxIncluded).mockReturnValue(true);
		await screen.getByRole('button', { name: 'open' }).click();
		await expect
			.element(page.getByText(m.topup_confirm_tax_included(), { exact: false }))
			.toBeVisible();
		await expect
			.element(page.getByText(m.topup_confirm_tax_exempt(), { exact: false }))
			.not.toBeInTheDocument();
	});

	it('価格・条件・規約類のリンクを出す', async () => {
		const screen = await render(Harness);

		await screen.getByRole('button', { name: 'open' }).click();

		await expect.element(page.getByText('300')).toBeVisible();
		await expect.element(page.getByText(m.topup_confirm_term_amount())).toBeVisible();
		await expect.element(page.getByText(m.topup_confirm_term_no_refund())).toBeVisible();
		await expect.element(page.getByText(m.topup_confirm_term_delete())).toBeVisible();
		await expect
			.element(page.getByRole('link', { name: new RegExp(m.terms_title()) }))
			.toHaveAttribute('href', '/terms');
		await expect
			.element(page.getByRole('link', { name: new RegExp(m.privacy_title()) }))
			.toHaveAttribute('href', '/privacy');
		await expect
			.element(page.getByRole('link', { name: new RegExp(m.tokushoho_title()) }))
			.toHaveAttribute('href', '/tokushoho');
	});

	it('同意すると Checkout の URL へ遷移する', async () => {
		vi.mocked(startOcrTopupCheckout).mockResolvedValue({
			ok: true,
			data: { checkoutUrl: 'https://checkout.stripe.test/session' }
		});
		const screen = await render(Harness);

		await screen.getByRole('button', { name: 'open' }).click();
		await page.getByRole('button', { name: m.topup_confirm_agree_button() }).click();

		await vi.waitFor(() =>
			expect(navigateTo).toHaveBeenCalledWith('https://checkout.stripe.test/session')
		);
	});

	it('Checkout の作成に失敗したら、確認ダイアログを閉じて知らせる', async () => {
		vi.mocked(startOcrTopupCheckout).mockResolvedValue({
			ok: false,
			message: '通信エラーが発生しました'
		});
		const screen = await render(Harness);

		await screen.getByRole('button', { name: 'open' }).click();
		await page.getByRole('button', { name: m.topup_confirm_agree_button() }).click();

		await expect.element(page.getByText('通信エラーが発生しました')).toBeVisible();
		await expect
			.element(page.getByRole('button', { name: m.topup_confirm_agree_button() }))
			.not.toBeInTheDocument();
		expect(navigateTo).not.toHaveBeenCalled();
	});
});
