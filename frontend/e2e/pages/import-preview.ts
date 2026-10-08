import type { Locator, Page } from '@playwright/test';
import { ja } from '../helpers';

/** 取り込み内容の確認 (import-preview.svelte)。写真で記録のページと CSV の取り込みで共通。
 * 一覧と確認は同じ画面に同時に出ないので、画面の中の行 (listitem) をそのまま探す。 */
export class ImportPreview {
	constructor(private readonly page: Page) {}

	get heading(): Locator {
		return this.page.getByRole('heading', { name: ja.import_preview_title });
	}

	get confirmButton(): Locator {
		return this.page.getByRole('button', { name: ja.import_preview_confirm_button, exact: true });
	}

	/** ほかで記録が変わったので出し直した、という案内。 */
	get changedElsewhereNotice(): Locator {
		return this.page.getByText(ja.import_preview_changed_elsewhere, { exact: true });
	}

	/** 消える記録があるときの注意書き。 */
	get deletionWarning(): Locator {
		return this.page.getByText(ja.import_preview_deletion_warning);
	}

	/** `text` を含む行。 */
	row(text: string): Locator {
		return this.page.getByRole('listitem').filter({ hasText: text });
	}
}
