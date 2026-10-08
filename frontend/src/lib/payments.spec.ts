import { beforeEach, describe, expect, it, vi } from 'vitest';

vi.mock('$lib/api/client', () => ({ client: { GET: vi.fn(), POST: vi.fn() } }));
import { client } from '$lib/api/client';
import { fetchOcrTopupResult, startOcrTopupCheckout } from '$lib/payments';
import { respond } from '$lib/api/client.test-helpers';

beforeEach(() => {
	vi.mocked(client.GET).mockReset();
	vi.mocked(client.POST).mockReset();
});

describe('fetchOcrTopupResult', () => {
	it('sessionId をクエリに渡す', async () => {
		vi.mocked(client.GET).mockReturnValue(respond(200, { status: 'processing' }));
		const result = await fetchOcrTopupResult('cs_test_123');
		expect(client.GET).toHaveBeenCalledWith('/api/v1/payments/ocr-topup/result', {
			params: { query: { sessionId: 'cs_test_123' } }
		});
		expect(result).toEqual({ ok: true, data: { status: 'processing' } });
	});

	it('404 は not_found の失敗として返す', async () => {
		vi.mocked(client.GET).mockReturnValue(respond(404, { error: { code: 'not_found' } }));
		const result = await fetchOcrTopupResult('cs_missing');
		expect(result.ok).toBe(false);
		if (!result.ok) expect(result.code).toBe('not_found');
	});
});

describe('startOcrTopupCheckout', () => {
	it('成功すると checkoutUrl を返す', async () => {
		vi.mocked(client.POST).mockReturnValue(
			respond(200, { checkoutUrl: 'https://checkout.stripe/x' })
		);
		const result = await startOcrTopupCheckout();
		expect(result).toEqual({ ok: true, data: { checkoutUrl: 'https://checkout.stripe/x' } });
	});
});
