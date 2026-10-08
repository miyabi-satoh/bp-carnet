import { expect, type Locator, type Page } from '@playwright/test';
import { ja } from '../helpers';
import { PhotoLightbox } from './photo-lightbox';

/** 読み取りに使った写真の枠 (photo-preview.svelte)。写真で記録のページと、行を直すシートに置かれる。 */
export class PhotoPreview {
	/** `scope` は枠を探す範囲 (ページ全体、またはシート)。 */
	constructor(
		private readonly page: Page,
		private readonly scope: Page | Locator
	) {}

	/** 枠の中の写真。ページ全体から探すときは、全画面を開いている間は全画面の写真も同じ名前で当たる。 */
	get image(): Locator {
		return this.scope.getByRole('img', { name: ja.photo_page_preview_alt });
	}

	/** 写真を枠の大きさに切り取っている要素。全画面を開閉する動きの起点になる。 */
	get view(): Locator {
		return this.image.locator('..');
	}

	/** 全画面で出す拡大ボタン。 */
	get openButton(): Locator {
		return this.scope.getByRole('button', { name: ja.photo_page_preview_open_label, exact: true });
	}

	/** 拡大ボタンで全画面を開く。 */
	async openLightbox(): Promise<PhotoLightbox> {
		const lightbox = new PhotoLightbox(this.page);
		await this.openButton.click();
		await expect(lightbox.root).toBeVisible();
		return lightbox;
	}
}
