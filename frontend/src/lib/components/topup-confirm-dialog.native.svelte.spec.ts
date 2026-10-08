// アプリでの買い足し (アプリ内課金、docs/payments.md)。
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { page } from 'vitest/browser';
import { render } from 'vitest-browser-svelte';
import * as m from '$lib/paraglide/messages.js';

vi.mock('$app/paths', () => ({ resolve: (path: string) => path }));
vi.mock('$lib/native-app', () => ({ isNativeApp: true }));
vi.mock('$lib/payments', () => ({ startOcrTopupCheckout: vi.fn() }));
vi.mock('$lib/app-store-purchase', () => ({ purchaseTopup: vi.fn(), topupDisplayPrice: vi.fn() }));
vi.mock('$lib/auth', () => ({
	fetchContactUrl: vi.fn(async () => 'https://example.com/contact')
}));
import { purchaseTopup, topupDisplayPrice } from '$lib/app-store-purchase';
import { fetchContactUrl } from '$lib/auth';
import { startOcrTopupCheckout } from '$lib/payments';
import Harness from './topup-confirm-dialog.test-harness.svelte';

beforeEach(() => {
	vi.mocked(purchaseTopup).mockReset();
	vi.mocked(topupDisplayPrice).mockReset().mockResolvedValue('¥300');
});

describe('TopupConfirmDialog (アプリ)', () => {
	it('App Store の価格を出し、返金は Apple の定めによると出す', async () => {
		const screen = await render(Harness);

		await screen.getByRole('button', { name: 'open' }).click();

		await expect.element(page.getByText('¥300')).toBeVisible();
		await expect.element(page.getByText(m.topup_confirm_term_refund_apple())).toBeVisible();
		await expect.element(page.getByText(m.topup_confirm_term_no_refund())).not.toBeInTheDocument();
		await expect
			.element(page.getByText(m.topup_confirm_tax_exempt(), { exact: false }))
			.not.toBeInTheDocument();
		await expect
			.element(page.getByText(m.topup_confirm_tax_included(), { exact: false }))
			.not.toBeInTheDocument();
	});

	it('価格が取れなければ買えない', async () => {
		vi.mocked(topupDisplayPrice).mockResolvedValue(null);
		const screen = await render(Harness);

		await screen.getByRole('button', { name: 'open' }).click();

		await expect.element(page.getByText(m.topup_confirm_app_unavailable())).toBeVisible();
		await expect
			.element(page.getByRole('button', { name: m.topup_confirm_agree_button() }))
			.toBeDisabled();
	});

	it('問い合わせ先を取り終えるまでは買えない', async () => {
		vi.mocked(fetchContactUrl).mockReturnValueOnce(new Promise(() => {}));
		const screen = await render(Harness);

		await screen.getByRole('button', { name: 'open' }).click();

		await expect.element(page.getByText(m.topup_confirm_app_price_loading())).toBeVisible();
		await expect
			.element(page.getByRole('button', { name: m.topup_confirm_agree_button() }))
			.toBeDisabled();
	});

	it('買えたら Stripe に進まず、枠を読み直して知らせる', async () => {
		vi.mocked(purchaseTopup).mockResolvedValue({ kind: 'granted' });
		const onpurchased = vi.fn();
		const screen = await render(Harness, { onpurchased });

		await screen.getByRole('button', { name: 'open' }).click();
		await page.getByRole('button', { name: m.topup_confirm_agree_button() }).click();

		await expect.element(page.getByText(m.topup_confirm_app_succeeded_title())).toBeVisible();
		expect(onpurchased).toHaveBeenCalledOnce();
		expect(startOcrTopupCheckout).not.toHaveBeenCalled();
	});

	it('反映が後になるときは、次に開いたときに反映すると知らせる', async () => {
		vi.mocked(purchaseTopup).mockResolvedValue({ kind: 'deferred' });
		const onpurchased = vi.fn();
		const screen = await render(Harness, { onpurchased });

		await screen.getByRole('button', { name: 'open' }).click();
		await page.getByRole('button', { name: m.topup_confirm_agree_button() }).click();

		await expect.element(page.getByText(m.topup_confirm_app_deferred_description())).toBeVisible();
		expect(onpurchased).not.toHaveBeenCalled();
	});

	it('サーバーが足さなかったら、運営者の問い合わせフォームへ案内する', async () => {
		vi.mocked(purchaseTopup).mockResolvedValue({ kind: 'rejected' });
		const screen = await render(Harness);

		await screen.getByRole('button', { name: 'open' }).click();
		await page.getByRole('button', { name: m.topup_confirm_agree_button() }).click();

		await expect
			.element(
				page.getByText(m.topup_confirm_app_rejected_description() + m.common_contact_operator())
			)
			.toBeVisible();
		await expect
			.element(page.getByRole('link', { name: m.common_contact_form_link(), exact: false }))
			.toHaveAttribute('href', 'https://example.com/contact');
	});

	it('購入シートを閉じたら、確認ダイアログに戻る', async () => {
		vi.mocked(purchaseTopup).mockResolvedValue({ kind: 'cancelled' });
		const screen = await render(Harness);

		await screen.getByRole('button', { name: 'open' }).click();
		await page.getByRole('button', { name: m.topup_confirm_agree_button() }).click();

		await expect
			.element(page.getByRole('button', { name: m.topup_confirm_agree_button() }))
			.toBeEnabled();
	});
});
