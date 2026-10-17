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

	/** 写真の外の、何も無い所 (閉じるボタンと同じ帯の左端) を押す。 */
	async clickOutsidePhoto(): Promise<void> {
		await this.root.click({ position: { x: 8, y: 8 } });
	}
}
