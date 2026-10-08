import { App } from '@capacitor/app';
import { registerPlugin } from '@capacitor/core';
import { client } from '$lib/api/client';
import { unwrapRequest } from '$lib/api/errors';
import type { components } from '$lib/api/schema';

/** アプリ内課金 (`mobile/plugins/app-store-purchase`、docs/payments.md)。 */
interface AppStorePurchasePlugin {
	product(options: { productId: string }): Promise<{ displayPrice: string }>;
	purchase(options: {
		productId: string;
		appAccountToken: string;
	}): Promise<({ result: 'purchased' } & StoreTransaction) | { result: 'pending' | 'cancelled' }>;
	unfinishedTransactions(): Promise<{ transactions: StoreTransaction[] }>;
	finish(options: { transactionId: string }): Promise<void>;
	debugRefundAvailable(): Promise<{ available: boolean }>;
	debugRefundRequest(): Promise<{ status: 'success' | 'cancelled' }>;
	addListener(
		event: 'transaction',
		listener: (transaction: StoreTransaction) => void
	): Promise<unknown>;
}

/** StoreKit の取引。`environment` は `Production`・`Sandbox`・`Xcode`。 */
interface StoreTransaction {
	transactionId: string;
	environment: string;
}

type AppleTransactionStatus = components['schemas']['AppStoreTransactionStatus'];

const AppStorePurchase = registerPlugin<AppStorePurchasePlugin>('AppStorePurchase');

/** 買い足しの商品 (App Store Connect の製品 ID。サーバーの `app_store::PRODUCT_ID` と同じ)。 */
const TOPUP_PRODUCT_ID = 'com.amiiby.bpcarnet.ocr_topup';

/** 買い足しの表示用の価格 (StoreKit の `displayPrice`)。取れなければ `null` (買えない)。 */
export async function topupDisplayPrice(): Promise<string | null> {
	try {
		return (await AppStorePurchase.product({ productId: TOPUP_PRODUCT_ID })).displayPrice;
	} catch {
		return null;
	}
}

/** 買い足しの結果。
 * - `granted`: 枠を足した (足し済みを含む)。
 * - `deferred`: 買えたが、今は反映できない。取引を終えずに残し、次の起動で送り直す。
 * - `rejected`: 買えたが、サーバーが足さないと決めた。
 * - `pending`: 承認待ち (ファミリーの承認と購入のリクエストなど)。
 * - `cancelled`: 利用者が購入シートを閉じた。
 * - `failed`: 購入シートを出せなかった (`message` は表示用)。 */
export type TopupPurchaseOutcome =
	| { kind: 'granted' | 'deferred' | 'rejected' | 'pending' | 'cancelled' }
	| { kind: 'failed'; message: string };

/** 購入シートを出し、買えたらサーバーに送って枠に反映する。 */
export async function purchaseTopup(): Promise<TopupPurchaseOutcome> {
	const token = await unwrapRequest(client.POST('/api/v1/payments/apple/account-token'));
	if (!token.ok) return { kind: 'failed', message: token.message };
	let purchase: Awaited<ReturnType<AppStorePurchasePlugin['purchase']>>;
	try {
		purchase = await AppStorePurchase.purchase({
			productId: TOPUP_PRODUCT_ID,
			appAccountToken: token.data.appAccountToken
		});
	} catch (error) {
		return { kind: 'failed', message: error instanceof Error ? error.message : String(error) };
	}
	if (purchase.result !== 'purchased') return { kind: purchase.result };
	switch (await reconcile(purchase)) {
		case 'granted':
		case 'already_granted':
			return { kind: 'granted' };
		case 'rejected':
			return { kind: 'rejected' };
		case null:
			return { kind: 'deferred' };
	}
}

/** サーバーに送っている最中の取引。起動時の取り直しと届いた取引が重なったら、送らずに先の答えを待つ。 */
const inFlight = new Map<string, Promise<AppleTransactionStatus | null>>();

/** 取引をサーバーに送り、答えが出たら終える。送れない・今は確かめられない (503・429 など) なら
 * 終えずに残し、`null` を返す (次に送るときに送り直す)。 */
function reconcile(transaction: StoreTransaction): Promise<AppleTransactionStatus | null> {
	const id = transaction.transactionId;
	let pending = inFlight.get(id);
	if (!pending) {
		pending = send(transaction).finally(() => inFlight.delete(id));
		inFlight.set(id, pending);
	}
	return pending;
}

async function send(transaction: StoreTransaction): Promise<AppleTransactionStatus | null> {
	const result = await unwrapRequest(
		client.POST('/api/v1/payments/apple/transactions', {
			// プラグインの結果には、ほかの項目 (`result`) も入っているので、送るものだけを取り出す。
			body: { transactionId: transaction.transactionId, environment: transaction.environment }
		})
	);
	if (!result.ok) return null;
	// 終えられなくても、サーバーの答えは出ている。残った取引は次に送ったとき、足し済みとして終える。
	await AppStorePurchase.finish({ transactionId: transaction.transactionId }).catch(() => {});
	return result.data.status;
}

let syncStarted = false;

/** 終えていない取引 (前に送れなかったもの) と、起動中に届く取引 (承認待ちが承認された等) を
 * サーバーに送る。ログインしてから1回だけ呼ぶ (送るにはログインが要るため)。
 * 終えていない取引は、アプリに戻るたびにも送り直す。iOS はプロセスを残したまま再開させることが多く、
 * 起動のときだけでは「次に開いたときに反映します」にならないため。 */
export function startTopupSync(): void {
	if (syncStarted) return;
	syncStarted = true;
	void AppStorePurchase.addListener('transaction', (transaction) => void reconcile(transaction));
	const sendUnfinished = () =>
		void AppStorePurchase.unfinishedTransactions().then(
			({ transactions }) => Promise.all(transactions.map(reconcile)),
			() => {}
		);
	sendUnfinished();
	void App.addListener('resume', sendUnfinished);
}

/** 開発用の返金の申し出を使えるか (Xcode から入れた Debug ビルドだけ)。 */
export async function debugRefundAvailable(): Promise<boolean> {
	try {
		return (await AppStorePurchase.debugRefundAvailable()).available;
	} catch {
		return false;
	}
}

/** 開発用。この端末で最後に買った取引の返金を申し出る (Sandbox で返金の通知を確かめるため)。
 * 失敗したら表示用の文を返す。 */
export async function requestDebugRefund(): Promise<string | null> {
	try {
		await AppStorePurchase.debugRefundRequest();
		return null;
	} catch (error) {
		return error instanceof Error ? error.message : String(error);
	}
}
