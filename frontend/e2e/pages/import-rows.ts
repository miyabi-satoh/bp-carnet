import { expect, type Locator, type Page } from '@playwright/test';
import { ja } from '../helpers';
import { RecordFormSheet } from './record-form-sheet';

/** 取り込む前の記録の一覧 (import-rows.svelte)。写真で記録のページと CSV の取り込みで共通。
 * 一覧と確認は同じ画面に同時に出ないので、画面の中の行 (listitem) をそのまま探す。 */
export class ImportRows {
	constructor(private readonly page: Page) {}

	get heading(): Locator {
		return this.page.getByRole('heading', { name: ja.import_rows_title, exact: true });
	}

	get description(): Locator {
		return this.page.getByText(ja.import_rows_description, { exact: true });
	}

	/** 「{total}件中{kept}件を取り込みます」。 */
	count(total: number, kept: number): Locator {
		return this.page.getByText(
			ja.import_rows_count.replace('{total}', String(total)).replace('{kept}', String(kept)),
			{ exact: true }
		);
	}

	/** 取り込める行が無いときの説明。 */
	get noneKeptDescription(): Locator {
		return this.page.getByText(ja.import_rows_description_none);
	}

	get items(): Locator {
		return this.page.getByRole('listitem');
	}

	/** `text` を含む行。 */
	row(text: string): Locator {
		return this.items.filter({ hasText: text });
	}

	/** 取り込むかのチェック。`row` を渡すとその行の中だけを探す。 */
	keepCheckbox(row?: Locator): Locator {
		return (row ?? this.page).getByRole('checkbox', { name: ja.record_row_keep_label });
	}

	get needsReviewFilter(): Locator {
		return this.page.getByRole('button', { name: ja.import_rows_needs_review_filter, exact: true });
	}

	/** `row` の直すボタン。 */
	editButton(row: Locator): Locator {
		return row.getByRole('button');
	}

	/** `row` の直すボタンからシートを開く。 */
	async openRow(row: Locator): Promise<RecordFormSheet> {
		await this.editButton(row).click();
		return new RecordFormSheet(this.page, 'edit');
	}

	/** `row` の上の血圧を `systolic` に直して更新し、シートが閉じるまで待つ。 */
	async fixSystolic(row: Locator, systolic: string): Promise<void> {
		const sheet = await this.openRow(row);
		await sheet.fields.systolic.fill(systolic);
		await sheet.updateButton.click();
		await expect(sheet.root).toBeHidden();
	}
}
