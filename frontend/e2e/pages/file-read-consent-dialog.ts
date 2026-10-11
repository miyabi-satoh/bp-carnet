import type { Locator, Page } from '@playwright/test';
import { ja } from '../helpers';

/** 他のアプリのファイルを読み取る前の、送るものを示して同意を得るダイアログ (file-read-consent-dialog.svelte)。 */
export class FileReadConsentDialog {
	readonly root: Locator;

	constructor(page: Page) {
		this.root = page.getByRole('dialog', { name: ja.file_read_consent_title });
	}

	get agreeButton(): Locator {
		return this.root.getByRole('button', { name: ja.file_read_consent_agree_button, exact: true });
	}

	get declineButton(): Locator {
		return this.root.getByRole('button', {
			name: ja.file_read_consent_decline_button,
			exact: true
		});
	}
}
