// アプリ内課金の購入と取り直し (docs/payments.md)。
import { beforeEach, describe, expect, it, vi } from 'vitest';

const plugin = vi.hoisted(() => ({
	product: vi.fn(),
	purchase: vi.fn(),
	unfinishedTransactions: vi.fn(),
	finish: vi.fn(),
	addListener: vi.fn(),
	debugRefundAvailable: vi.fn(),
	debugRefundRequest: vi.fn()
}));
vi.mock('@capacitor/core', async (importOriginal) => ({
	...(await importOriginal<typeof import('@capacitor/core')>()),
	registerPlugin: () => plugin
}));
vi.mock('$lib/api/client', () => ({ client: { POST: vi.fn() } }));
const app = vi.hoisted(() => ({ addListener: vi.fn() }));
vi.mock('@capacitor/app', () => ({ App: app }));

import { client } from '$lib/api/client';
import { respond } from '$lib/api/client.test-helpers';
import {
	debugRefundAvailable,
	purchaseTopup,
	requestDebugRefund,
	startTopupSync,
	topupDisplayPrice
} from './app-store-purchase';

const TOKEN = '0f8fad5b-d9cb-469f-a165-70867728950e';

/** アカウントのトークンを返し、取引の送信には `transactionResponse` を返す。 */
function serverAnswers(transactionResponse: ReturnType<typeof respond>) {
	vi.mocked(client.POST).mockImplementation(((path: string) =>
		path === '/api/v1/payments/apple/account-token'
			? respond(200, { appAccountToken: TOKEN })
			: transactionResponse) as typeof client.POST);
}

beforeEach(() => {
	vi.clearAllMocks();
	plugin.finish.mockResolvedValue(undefined);
	plugin.addListener.mockResolvedValue({});
});

describe('topupDisplayPrice', () => {
	it('StoreKit の表示用の価格を返し、取れなければ null', async () => {
		plugin.product.mockResolvedValueOnce({ displayPrice: '¥300' });
		expect(await topupDisplayPrice()).toBe('¥300');
		plugin.product.mockRejectedValueOnce(new Error('offline'));
		expect(await topupDisplayPrice()).toBeNull();
	});
});

describe('開発用の返金の申し出', () => {
	it('Debug ビルドでだけ使え、プラグインが無い (古い版・ウェブ) なら使えない', async () => {
		plugin.debugRefundAvailable.mockResolvedValueOnce({ available: true });
		expect(await debugRefundAvailable()).toBe(true);
		plugin.debugRefundAvailable.mockResolvedValueOnce({ available: false });
		expect(await debugRefundAvailable()).toBe(false);
		plugin.debugRefundAvailable.mockRejectedValueOnce(new Error('not implemented'));
		expect(await debugRefundAvailable()).toBe(false);
	});

	it('申し出られたら null、失敗したら表示用の文を返す', async () => {
		plugin.debugRefundRequest.mockResolvedValueOnce({ status: 'success' });
		expect(await requestDebugRefund()).toBeNull();
		plugin.debugRefundRequest.mockRejectedValueOnce(new Error('この端末で買った取引がありません'));
		expect(await requestDebugRefund()).toBe('この端末で買った取引がありません');
	});
});

describe('purchaseTopup', () => {
	it('アカウントのトークンを付けて買い、サーバーが足したら取引を終える', async () => {
		serverAnswers(respond(200, { status: 'granted' }));
		plugin.purchase.mockResolvedValue({
			result: 'purchased',
			transactionId: '2000000001',
			environment: 'Production'
		});

		expect(await purchaseTopup()).toEqual({ kind: 'granted' });
		expect(plugin.purchase).toHaveBeenCalledWith({
			productId: 'com.amiiby.bpcarnet.ocr_topup',
			appAccountToken: TOKEN
		});
		expect(client.POST).toHaveBeenCalledWith('/api/v1/payments/apple/transactions', {
			body: { transactionId: '2000000001', environment: 'Production' }
		});
		expect(plugin.finish).toHaveBeenCalledWith({ transactionId: '2000000001' });
	});

	it('サーバーが足さないと決めた取引も終える', async () => {
		serverAnswers(respond(200, { status: 'rejected' }));
		plugin.purchase.mockResolvedValue({
			result: 'purchased',
			transactionId: '2000000002',
			environment: 'Xcode'
		});

		expect(await purchaseTopup()).toEqual({ kind: 'rejected' });
		expect(plugin.finish).toHaveBeenCalledWith({ transactionId: '2000000002' });
	});

	it('取引を終えられなくても、サーバーの答えを返す', async () => {
		serverAnswers(respond(200, { status: 'granted' }));
		plugin.finish.mockRejectedValue(new Error('finish failed'));
		plugin.purchase.mockResolvedValue({
			result: 'purchased',
			transactionId: '2000000007',
			environment: 'Production'
		});

		expect(await purchaseTopup()).toEqual({ kind: 'granted' });
	});

	// 起動中に届く取引の送信と重なっても、「反映できなかった」とは出さない。
	it('同じ取引を送っている最中なら、送らずにその答えを待つ', async () => {
		// 起動中に届く取引を受けるため、ほかのテストと別の状態 (`startTopupSync` を呼んでいない) で読み込む。
		vi.resetModules();
		const fresh = await import('./app-store-purchase');
		const answer = Promise.withResolvers<unknown>();
		vi.mocked(client.POST).mockImplementation(((path: string) =>
			path === '/api/v1/payments/apple/account-token'
				? respond(200, { appAccountToken: TOKEN })
				: answer.promise) as typeof client.POST);
		plugin.unfinishedTransactions.mockResolvedValue({ transactions: [] });
		fresh.startTopupSync();
		const listener = plugin.addListener.mock.calls[0][1];
		listener({ transactionId: '2000000008', environment: 'Production' });
		plugin.purchase.mockResolvedValue({
			result: 'purchased',
			transactionId: '2000000008',
			environment: 'Production'
		});

		const outcome = fresh.purchaseTopup();
		await vi.waitFor(() => expect(plugin.purchase).toHaveBeenCalled());
		answer.resolve(await respond(200, { status: 'granted' }));

		expect(await outcome).toEqual({ kind: 'granted' });
		const sent = vi
			.mocked(client.POST)
			.mock.calls.filter((call) => call[0] === '/api/v1/payments/apple/transactions');
		expect(sent).toHaveLength(1);
	});

	// 終えると次の起動で送り直されず、支払い済みの購入を取りこぼす。
	it('今は確かめられないときは取引を終えない', async () => {
		serverAnswers(respond(503, { error: { code: 'app_store_unavailable', message: 'x' } }));
		plugin.purchase.mockResolvedValue({
			result: 'purchased',
			transactionId: '2000000003',
			environment: 'Production'
		});

		expect(await purchaseTopup()).toEqual({ kind: 'deferred' });
		expect(plugin.finish).not.toHaveBeenCalled();
	});

	it('承認待ち・閉じたときはサーバーに送らない', async () => {
		serverAnswers(respond(200, { status: 'granted' }));
		plugin.purchase.mockResolvedValueOnce({ result: 'pending' });
		expect(await purchaseTopup()).toEqual({ kind: 'pending' });
		plugin.purchase.mockResolvedValueOnce({ result: 'cancelled' });
		expect(await purchaseTopup()).toEqual({ kind: 'cancelled' });
		expect(client.POST).not.toHaveBeenCalledWith(
			'/api/v1/payments/apple/transactions',
			expect.anything()
		);
	});

	it('トークンを受け取れなければ購入シートを出さない', async () => {
		vi.mocked(client.POST).mockReturnValue(respond(500));
		expect((await purchaseTopup()).kind).toBe('failed');
		expect(plugin.purchase).not.toHaveBeenCalled();
	});
});

describe('startTopupSync', () => {
	it('終えていない取引と起動中に届く取引を送り、答えが出たものを終える', async () => {
		serverAnswers(respond(200, { status: 'already_granted' }));
		plugin.unfinishedTransactions.mockResolvedValue({
			transactions: [{ transactionId: '2000000004', environment: 'Sandbox' }]
		});

		startTopupSync();
		startTopupSync();

		await vi.waitFor(() =>
			expect(plugin.finish).toHaveBeenCalledWith({ transactionId: '2000000004' })
		);
		expect(plugin.unfinishedTransactions).toHaveBeenCalledTimes(1);
		const listener = plugin.addListener.mock.calls[0][1];
		listener({ transactionId: '2000000005', environment: 'Production' });
		await vi.waitFor(() =>
			expect(plugin.finish).toHaveBeenCalledWith({ transactionId: '2000000005' })
		);

		// アプリに戻ったら、終えていない取引を送り直す。
		plugin.unfinishedTransactions.mockResolvedValue({
			transactions: [{ transactionId: '2000000006', environment: 'Production' }]
		});
		const [event, onResume] = app.addListener.mock.calls[0];
		expect(event).toBe('resume');
		onResume();
		await vi.waitFor(() =>
			expect(plugin.finish).toHaveBeenCalledWith({ transactionId: '2000000006' })
		);
	});
});
