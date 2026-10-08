import type { Locator, Page } from '@playwright/test';
import { ja } from '../helpers';

/** 記録の削除の確認 (record-delete-dialog.svelte)。ホームの記録カードのゴミ箱から開く。 */
export class RecordDeleteDialog {
	readonly root: Locator;

	constructor(private readonly page: Page) {
		this.root = page.getByRole('alertdialog', { name: ja.record_delete_confirm_title });
	}

	get deleteButton(): Locator {
		return this.root.getByRole('button', { name: ja.common_delete_button, exact: true });
	}

	get cancelButton(): Locator {
		return this.root.getByRole('button', { name: ja.common_cancel_button, exact: true });
	}

	/** 戻せない操作の確認は × を出さない。無いことを確かめるために探す。 */
	get closeButton(): Locator {
		return this.root.getByRole('button', { name: ja.common_close_button, exact: true });
	}

	/** 消せなかったときのお知らせ。 */
	get errorNotice(): Locator {
		return this.page.getByRole('alertdialog', { name: ja.record_delete_error_title });
	}
}
