import type { Locator, Page } from '@playwright/test';
import { ja } from '../helpers';

/** 写真を初めて読み取る前の、送り先を示して同意を得るダイアログ (ocr-consent-dialog.svelte)。 */
export class OcrConsentDialog {
	readonly root: Locator;

	constructor(page: Page) {
		this.root = page.getByRole('dialog', { name: ja.ocr_consent_title });
	}

	/** 送り先 (Google の Gemini API) の行。 */
	get recipient(): Locator {
		return this.root.getByText(ja.ocr_consent_item_recipient, { exact: true });
	}

	get agreeButton(): Locator {
		return this.root.getByRole('button', { name: ja.ocr_consent_agree_button, exact: true });
	}

	get declineButton(): Locator {
		return this.root.getByRole('button', { name: ja.ocr_consent_decline_button, exact: true });
	}
}
