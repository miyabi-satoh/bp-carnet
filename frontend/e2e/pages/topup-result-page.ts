import type { Locator, Page } from '@playwright/test';
import { ja } from '../helpers';

/** Stripe から戻った後の、買い足しの結果のページ (`/payments/ocr-topup/result`)。 */
export class TopupResultPage {
	constructor(private readonly page: Page) {}

	/** Stripe から戻ったときと同じく、Checkout の `sessionId` を付けて開く。 */
	async open(sessionId: string): Promise<void> {
		await this.page.goto(`/payments/ocr-topup/result?session_id=${sessionId}`);
	}

	get succeededTitle(): Locator {
		return this.page.getByText(ja.topup_result_succeeded_title, { exact: true });
	}

	/** 「写真で記録に戻る」。 */
	get backToPhotoButton(): Locator {
		return this.page.getByRole('button', {
			name: ja.topup_result_back_to_photo_button,
			exact: true
		});
	}
}
