import type { Locator, Page } from '@playwright/test';
import { ja } from '../helpers';
import { LeaveConfirmDialog } from './leave-confirm-dialog';
import { PhotoPreview } from './photo-preview';
import { RecordFields } from './record-fields';

/** 記録フォームのシート (record-form-sheet.svelte)。ホームの「手動で入力...」・記録カードと、取り込みの行から開く。 */
export class RecordFormSheet {
	readonly root: Locator;
	readonly fields: RecordFields;
	/** 写真で記録の行を直すシートでだけ、見出しの下に出る写真の枠 (import-row-sheet.svelte)。 */
	readonly photoPreview: PhotoPreview;

	constructor(
		private readonly page: Page,
		mode: 'add' | 'edit'
	) {
		this.root = page.getByRole('dialog', {
			name: mode === 'add' ? ja.record_form_dialog_title : ja.record_form_dialog_edit_title
		});
		this.fields = new RecordFields(this.root);
		this.photoPreview = new PhotoPreview(page, this.root);
	}

	get createButton(): Locator {
		return this.root.getByRole('button', {
			name: ja.record_form_dialog_create_button,
			exact: true
		});
	}

	get updateButton(): Locator {
		return this.root.getByRole('button', {
			name: ja.record_form_dialog_update_button,
			exact: true
		});
	}

	/** ほかで記録が変わったので出し直した、という案内。 */
	get changedElsewhereNotice(): Locator {
		return this.root.getByText(ja.record_form_dialog_changed_elsewhere, { exact: true });
	}

	/** 入力を捨てて閉じるかの確認。 */
	get discardConfirm(): LeaveConfirmDialog {
		return new LeaveConfirmDialog(this.page, {
			title: ja.record_form_dialog_discard_title,
			leave: ja.record_form_dialog_discard_button
		});
	}

	/** 直すのに失敗したときのお知らせ。 */
	get errorNotice(): Locator {
		return this.page.getByRole('alertdialog', { name: ja.record_form_dialog_update_error_title });
	}
}
