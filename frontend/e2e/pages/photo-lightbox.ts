import type { Locator, Page } from '@playwright/test';
import { ja } from '../helpers';

/** 写真の拡大表示 (photo-lightbox.svelte)。 */
export class PhotoLightbox {
	readonly root: Locator;

	constructor(page: Page) {
		this.root = page.getByRole('dialog', { name: ja.photo_page_preview_alt });
	}

	get photo(): Locator {
		return this.root.getByRole('img', { name: ja.photo_page_preview_alt });
	}

	get closeButton(): Locator {
		return this.root.getByRole('button', { name: ja.common_close_button, exact: true });
	}

	get hint(): Locator {
		return this.root.getByText(ja.photo_lightbox_hint, { exact: true });
	}
}
