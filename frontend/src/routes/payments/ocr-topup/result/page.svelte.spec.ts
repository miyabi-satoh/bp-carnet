import { beforeEach, describe, expect, it, vi } from 'vitest';
import { render } from 'vitest-browser-svelte';
import * as m from '$lib/paraglide/messages.js';

vi.mock('$app/navigation', () => ({ goto: vi.fn() }));
vi.mock('$app/paths', () => ({ resolve: (path: string) => path }));
const mockPage = vi.hoisted(() => ({
	url: new URL('http://localhost/payments/ocr-topup/result')
}));
vi.mock('$app/state', () => ({ page: mockPage }));
vi.mock('$lib/payments', () => ({ fetchOcrTopupResult: vi.fn() }));
import { goto } from '$app/navigation';
import { fetchOcrTopupResult } from '$lib/payments';
import ResultPage from './+page.svelte';

function withSessionId(sessionId: string | null) {
	mockPage.url = new URL(
		sessionId === null
			? 'http://localhost/payments/ocr-topup/result'
			: `http://localhost/payments/ocr-topup/result?session_id=${sessionId}`
	);
}

beforeEach(() => {
	vi.mocked(goto).mockReset();
	vi.mocked(fetchOcrTopupResult).mockReset();
	withSessionId('cs_test_123');
});

describe('購入結果ページ', () => {
	it('session_id が無ければ、問い合わせずにエラー表示にする', async () => {
		withSessionId(null);
		const screen = await render(ResultPage);

		await expect.element(screen.getByText(m.topup_result_error_description())).toBeVisible();
		expect(fetchOcrTopupResult).not.toHaveBeenCalled();
	});

	it('成功したら完了表示にし、戻るボタンで写真で記録に戻る', async () => {
		vi.mocked(fetchOcrTopupResult).mockResolvedValue({ ok: true, data: { status: 'succeeded' } });
		const screen = await render(ResultPage);

		await expect.element(screen.getByText(m.topup_result_succeeded_title())).toBeVisible();
		expect(fetchOcrTopupResult).toHaveBeenCalledWith('cs_test_123');

		await screen.getByRole('button', { name: m.topup_result_back_to_photo_button() }).click();
		expect(goto).toHaveBeenCalledWith('/record/photo');
	});

	it('失敗したら、もう一度試す・戻るの両方のボタンを出す', async () => {
		vi.mocked(fetchOcrTopupResult).mockResolvedValue({ ok: true, data: { status: 'failed' } });
		const screen = await render(ResultPage);

		await expect.element(screen.getByText(m.topup_result_failed_title())).toBeVisible();
		await expect
			.element(screen.getByRole('button', { name: m.topup_result_retry_button() }))
			.toBeVisible();
		await expect
			.element(screen.getByRole('button', { name: m.topup_result_back_to_photo_button() }))
			.toBeVisible();
	});

	it('自分のセッションでない・存在しない (404) は確定した失敗として扱う', async () => {
		vi.mocked(fetchOcrTopupResult).mockResolvedValue({
			ok: false,
			message: '見つかりません',
			code: 'not_found'
		});
		const screen = await render(ResultPage);

		await expect.element(screen.getByText(m.topup_result_error_description())).toBeVisible();
	});

	it('processing の間はポーリングを続け、確定したら表示を切り替える', async () => {
		vi.useFakeTimers();
		try {
			vi.mocked(fetchOcrTopupResult)
				.mockResolvedValueOnce({ ok: true, data: { status: 'processing' } })
				.mockResolvedValueOnce({ ok: true, data: { status: 'succeeded' } });
			const screen = await render(ResultPage);

			await vi.waitFor(() => expect(fetchOcrTopupResult).toHaveBeenCalledTimes(1));
			await expect.element(screen.getByText(m.topup_result_processing_title())).toBeVisible();

			await vi.advanceTimersByTimeAsync(2000);
			await vi.waitFor(() => expect(fetchOcrTopupResult).toHaveBeenCalledTimes(2));
			await expect.element(screen.getByText(m.topup_result_succeeded_title())).toBeVisible();
		} finally {
			vi.useRealTimers();
		}
	});

	it('確定しないまま試行回数が尽きたら、確かめ直す・戻るの導線を出す', async () => {
		vi.useFakeTimers();
		try {
			vi.mocked(fetchOcrTopupResult).mockResolvedValue({
				ok: true,
				data: { status: 'processing' }
			});
			const screen = await render(ResultPage);

			await vi.waitFor(() => expect(fetchOcrTopupResult).toHaveBeenCalledTimes(1));
			for (let i = 0; i < 14; i++) {
				await vi.advanceTimersByTimeAsync(2000);
			}
			await vi.waitFor(() => expect(fetchOcrTopupResult).toHaveBeenCalledTimes(15));

			await expect.element(screen.getByText(m.topup_result_timed_out_description())).toBeVisible();
			await expect
				.element(screen.getByRole('button', { name: m.topup_result_check_again_button() }))
				.toBeVisible();
			await expect
				.element(screen.getByRole('button', { name: m.topup_result_back_to_photo_button() }))
				.toBeVisible();
		} finally {
			vi.useRealTimers();
		}
	});

	it('確認が打ち切られたら、もう一度確かめられ、届いていれば完了表示に切り替わる', async () => {
		vi.useFakeTimers();
		try {
			vi.mocked(fetchOcrTopupResult).mockResolvedValue({
				ok: true,
				data: { status: 'processing' }
			});
			const screen = await render(ResultPage);
			await vi.runAllTimersAsync();
			await expect.element(screen.getByText(m.topup_result_timed_out_description())).toBeVisible();

			vi.mocked(fetchOcrTopupResult).mockResolvedValue({ ok: true, data: { status: 'succeeded' } });
			await screen.getByRole('button', { name: m.topup_result_check_again_button() }).click();
			await expect.element(screen.getByText(m.topup_result_succeeded_title())).toBeVisible();
		} finally {
			vi.useRealTimers();
		}
	});
});
