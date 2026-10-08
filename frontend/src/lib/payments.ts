import { client } from '$lib/api/client';
import { type ApiResult, unwrapRequest } from '$lib/api/errors';
import type { components } from '$lib/api/schema';

/** 買い足しの結果 (`GET /payments/ocr-topup/result`) の状態。 */
export type OcrTopupResultStatus = components['schemas']['OcrTopupResultStatus'];

/** `POST /api/v1/payments/ocr-topup/checkout` — Stripe-hosted Checkout の URL を得る。
 * リクエストボディは無い (ユーザーとサーバー設定の Price から Session を作る)。成功したら
 * 呼び出し側が `checkoutUrl` へ遷移する。失敗は表示用の文言つきで返す。 */
export async function startOcrTopupCheckout(): Promise<
	ApiResult<{ data: { checkoutUrl: string } }>
> {
	return unwrapRequest(client.POST('/api/v1/payments/ocr-topup/checkout'));
}

/** `GET /api/v1/payments/ocr-topup/result?sessionId=…` — 成功URLの戻り先でポーリングする。
 * `processing` の間は結果画面で待ち、`succeeded`/`failed` で表示を切り替える。 */
export async function fetchOcrTopupResult(
	sessionId: string
): Promise<ApiResult<{ data: { status: OcrTopupResultStatus } }>> {
	return unwrapRequest(
		client.GET('/api/v1/payments/ocr-topup/result', {
			params: { query: { sessionId } }
		})
	);
}
