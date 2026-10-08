// アプリ (Capacitor) の中で開いた写真で記録の画面。アプリ内課金の購入経路は E2E (ウェブ) では通らないため、ここで確かめる。
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { page } from 'vitest/browser';
import { render } from 'vitest-browser-svelte';
import * as m from '$lib/paraglide/messages.js';

vi.mock('$app/navigation', () => ({
	afterNavigate: vi.fn(),
	beforeNavigate: vi.fn(),
	goto: vi.fn(),
	invalidateAll: vi.fn(),
	preloadCode: vi.fn(async () => {}),
	pushState: vi.fn(),
	replaceState: vi.fn()
}));
vi.mock('$app/paths', () => ({ asset: (path: string) => path, resolve: (path: string) => path }));
vi.mock('$app/state', () => ({
	navigating: { to: null },
	page: {
		url: new URL('http://localhost/record/photo'),
		state: {},
		data: { user: { timezone: 'Asia/Tokyo' } }
	}
}));
vi.mock('$lib/native-app', async (importOriginal) => ({
	...(await importOriginal<typeof import('$lib/native-app')>()),
	isNativeApp: true
}));
vi.mock('$lib/app-store-purchase', () => ({
	purchaseTopup: vi.fn(async () => ({ kind: 'granted' })),
	topupDisplayPrice: vi.fn(async () => '¥300')
}));
vi.mock('$lib/auth', async (importOriginal) => ({
	...(await importOriginal<typeof import('$lib/auth')>()),
	fetchContactUrl: vi.fn(async () => null)
}));
vi.mock('$lib/settings', async (importOriginal) => {
	const original = await importOriginal<typeof import('$lib/settings')>();
	return {
		...original,
		fetchPeriodThresholds: vi.fn(async () => original.DEFAULT_PERIOD_THRESHOLDS)
	};
});
vi.mock('$lib/photo-taken-at', () => ({ photoTakenAt: vi.fn(async () => null) }));
vi.mock('$lib/ocr', { spy: true });
import { extractFromPhoto, fetchOcrStatus, photoFileHandoff, type OcrStatus } from '$lib/ocr';
import PhotoPage from './+page.svelte';

const status = (remainingPercent: number): OcrStatus => ({
	enabled: true,
	quotas: [{ kind: 'free', purchasedAt: null, remainingPercent }],
	quotaLow: remainingPercent <= 20,
	topupAvailable: true,
	consented: true
});

beforeEach(() => {
	vi.spyOn(photoFileHandoff, 'take').mockReturnValue(
		new File([new Uint8Array([0xff, 0xd8, 0xff])], 'photo.jpg', { type: 'image/jpeg' })
	);
	vi.mocked(extractFromPhoto).mockResolvedValue({
		ok: false,
		code: 'ocr_budget_exhausted',
		message: m.error_ocr_budget_exhausted(),
		blocked: true
	});
});

describe('使い切りで読み取れなかった後の買い足し', () => {
	it('買えたら、選んだ写真のまま「読み取る」に戻る', async () => {
		// 画面を開いた時点では残りがあり、読み取りで使い切りと分かる (枠の表示の取り直しが遅れた場合)。
		vi.mocked(fetchOcrStatus).mockResolvedValueOnce(status(10)).mockResolvedValue(status(0));
		const screen = await render(PhotoPage);

		await screen.getByRole('button', { name: m.photo_page_read_button() }).click();
		await screen.getByRole('button', { name: m.photo_page_topup_button() }).click();

		vi.mocked(fetchOcrStatus).mockResolvedValue(status(100));
		await page.getByRole('button', { name: m.topup_confirm_agree_button() }).click();
		await expect.element(page.getByText(m.topup_confirm_app_succeeded_title())).toBeVisible();

		await expect
			.element(screen.getByRole('button', { name: m.photo_page_read_button() }))
			.toBeInTheDocument();
	});
});
