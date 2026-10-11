import type { Locator, Page } from '@playwright/test';
import { ja } from '../helpers';
import { FileReadConsentDialog } from './file-read-consent-dialog';
import { ImportPreview } from './import-preview';
import { ImportRows } from './import-rows';
import { LeaveConfirmDialog } from './leave-confirm-dialog';

/** ファイルからの取り込み (`/settings/import`)。読み込んだ行の一覧から、取り込み内容の確認へ進む。 */
export class ImportPage {
	readonly rows: ImportRows;
	readonly preview: ImportPreview;
	/** 行を直した後にページを離れようとしたときの確認。 */
	readonly leaveConfirm: LeaveConfirmDialog;
	/** 他のアプリのファイルを Gemini に送る前の確認。 */
	readonly fileReadConsent: FileReadConsentDialog;

	constructor(private readonly page: Page) {
		this.rows = new ImportRows(page);
		this.preview = new ImportPreview(page);
		this.fileReadConsent = new FileReadConsentDialog(page);
		this.leaveConfirm = new LeaveConfirmDialog(page, {
			title: ja.leave_confirm_dialog_title,
			leave: ja.leave_confirm_dialog_leave_button
		});
	}

	/** ファイルを選ぶ前と、読み込めなかった後に出る「ファイルを選ぶ」。 */
	get pickFileButton(): Locator {
		return this.page.getByRole('button', { name: ja.import_page_pick_file_button, exact: true });
	}

	/** 読み込めなかったときの文。 */
	error(message: string): Locator {
		return this.page.getByText(message, { exact: true });
	}

	get reviewButton(): Locator {
		return this.page.getByRole('button', { name: ja.common_review_button, exact: true });
	}

	/** 下端の「戻る」。見出しの戻るボタンと同じ名前なので、下端のボタンの帯 (screen-footer.svelte) の中から探す。 */
	get backButton(): Locator {
		return this.page
			.locator('[data-slot="screen-footer"]')
			.getByRole('button', { name: ja.common_back_button, exact: true });
	}
}
