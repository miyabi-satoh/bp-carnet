import type { Locator, Page } from '@playwright/test';
import { chooseFile, ja } from '../helpers';
import { HomePage } from './home-page';
import { ImportPreview } from './import-preview';
import { ImportRows } from './import-rows';
import { LeaveConfirmDialog } from './leave-confirm-dialog';
import { OcrConsentDialog } from './ocr-consent-dialog';
import { SameRecordDialog } from './same-record-dialog';
import { TopupConfirmDialog } from './topup-confirm-dialog';
import { PhotoLightbox } from './photo-lightbox';
import { PhotoPreview } from './photo-preview';
import { RecordFields } from './record-fields';

/** 写真で記録のページ (`/record/photo`)。血圧計の読み取り結果はページの中のフォーム、手帳の読み取り結果は行の一覧で出る。 */
export class PhotoPage {
	/** 血圧計の読み取り結果のフォーム。 */
	readonly fields: RecordFields;
	readonly rows: ImportRows;
	readonly preview: ImportPreview;
	readonly lightbox: PhotoLightbox;
	/** 読み取り中・読み取った後に、見出しの下に出る写真の枠。 */
	readonly photoPreview: PhotoPreview;
	/** 行を変えた後にページを離れようとしたときの確認。 */
	readonly leaveConfirm: LeaveConfirmDialog;
	/** 同じ日時・同じ値の記録がすでにあるときの、登録の前の確認。 */
	readonly sameRecord: SameRecordDialog;
	/** まだ同意していないときに、「読み取る」で出る同意のダイアログ。 */
	readonly consent: OcrConsentDialog;

	constructor(private readonly page: Page) {
		this.fields = new RecordFields(page);
		this.rows = new ImportRows(page);
		this.preview = new ImportPreview(page);
		this.lightbox = new PhotoLightbox(page);
		this.photoPreview = new PhotoPreview(page, page);
		this.leaveConfirm = new LeaveConfirmDialog(page, {
			title: ja.leave_confirm_dialog_title,
			leave: ja.leave_confirm_dialog_leave_button
		});
		this.sameRecord = new SameRecordDialog(page);
		this.consent = new OcrConsentDialog(page);
	}

	/** 写真を持たずに直接開く。 */
	async open(): Promise<void> {
		await this.page.goto('/record/photo');
	}

	get heading(): Locator {
		return this.page.getByRole('heading', { name: ja.photo_page_title });
	}

	/** 写真が無いまま開いたときの「写真を選ぶ」。無料枠を使い切っていると出ない。 */
	get pickPhotoButton(): Locator {
		return this.page.getByRole('button', { name: ja.photo_page_pick_photo_button, exact: true });
	}

	/** 写真が無いまま開いたときの「写真を選ぶ」で `file` を選び、読み取る。 */
	async pickPhoto(file: Parameters<typeof chooseFile>[2]): Promise<void> {
		await chooseFile(this.page, this.pickPhotoButton, file);
		await this.readButton.click();
	}

	/** 選んだ写真を確かめる段階の「読み取る」。押すまで読み取らない。 */
	get readButton(): Locator {
		return this.page.getByRole('button', { name: ja.photo_page_read_button, exact: true });
	}

	/** 血圧計の読み取り結果を登録するボタン。 */
	get createButton(): Locator {
		return this.page.getByRole('button', {
			name: ja.record_form_dialog_create_button,
			exact: true
		});
	}

	/** 読み取る前・読み取った後の「写真を選び直す」。 */
	get retakeButton(): Locator {
		return this.page.getByRole('button', { name: ja.photo_page_retake_button, exact: true });
	}

	/** 「写真を選び直す」で `file` を選び、読み取る。 */
	async retake(file: Parameters<typeof chooseFile>[2]): Promise<void> {
		await chooseFile(this.page, this.retakeButton, file);
		await this.readButton.click();
	}

	/** 読み取りに失敗したときのお知らせ。 */
	errorMessage(message: string): Locator {
		return this.page.getByText(message);
	}

	/** 読み取れないときの「数字で入力する」。ホーム画面へ戻って記録フォームを開く。 */
	get manualEntryButton(): Locator {
		return this.page.getByRole('button', { name: ja.photo_page_manual_entry_button, exact: true });
	}

	/** 読み取りに失敗したときの「もう一度」。 */
	get againButton(): Locator {
		return this.page.getByRole('button', { name: ja.common_again_button, exact: true });
	}

	/** 使い方のページの「写真が読み取れないとき」へのリンク。 */
	get troubleHelpLink(): Locator {
		return this.page.getByRole('link', { name: ja.help_trouble_photo_title, exact: true });
	}

	/** 読み取り結果のフォームの上の案内。信頼度が「低」なら `ja.photo_page_review_note_low`。 */
	reviewNote(message: string): Locator {
		return this.page.getByText(message, { exact: true });
	}

	get noValuesMessage(): Locator {
		return this.page.getByText(ja.photo_page_no_values, { exact: true });
	}

	/** 読み取れなかったときの、写っていたものに合わせた案内。 */
	noValuesHint(kind: 'monitor_none' | 'unknown'): Locator {
		const text =
			kind === 'monitor_none'
				? ja.photo_page_no_values_monitor_none
				: ja.photo_page_no_values_unknown;
		return this.page.getByText(text, { exact: true });
	}

	/** 残りの枠の見出し「無料の読み取り」(無料の枠を使っているとき)。 */
	get freeQuotaTitle(): Locator {
		return this.page.getByText(ja.ocr_quota_free_title, { exact: true });
	}

	/** 残りの枠の見出し「買い足した読み取り」(買い足した枠を使っているとき)。 */
	get paidQuotaTitle(): Locator {
		return this.page.getByText(ja.ocr_quota_paid_title, { exact: true });
	}

	/** 残りの枠の「残り N%」。 */
	quotaRemaining(percent: number): Locator {
		return this.page.getByText(ja.ocr_quota_remaining.replace('{percent}', String(percent)), {
			exact: true
		});
	}

	/** ほかにも枠が残っているときの、設定への案内「ほかの枠を見る」。 */
	get otherQuotasLink(): Locator {
		return this.page.getByRole('link', { name: ja.photo_page_other_quotas_link });
	}

	/** 無料枠を使い切ったときの、残りの枠の見出し (「無料の読み取り」と置き換わる)。 */
	get quotaExhaustedTitle(): Locator {
		return this.page.getByText(ja.photo_page_quota_exhausted_title, { exact: true });
	}

	/** 読み取りが無効なときの知らせ。 */
	get ocrDisabledMessage(): Locator {
		return this.page.getByText(ja.error_ocr_disabled, { exact: true });
	}

	/** 「読み取りを買い足す」のボタン。 */
	get topupButton(): Locator {
		return this.page.getByRole('button', { name: ja.photo_page_topup_button, exact: true });
	}

	/** 「読み取りを買い足す」で確認のダイアログを開く。 */
	async openTopup(): Promise<TopupConfirmDialog> {
		await this.topupButton.click();
		return new TopupConfirmDialog(this.page);
	}

	/** 使い切りの枠の、買い足せないときの次の一手 (数字での記録) の文。 */
	get quotaExhaustedManualNote(): Locator {
		return this.page.getByText(ja.photo_page_quota_exhausted_manual, { exact: true });
	}

	/** 手帳の読み取り結果に取り込める行が無いときの注意書き。 */
	get noneKeptNote(): Locator {
		return this.page.getByText(ja.photo_page_memo_none_kept_note);
	}

	/** 手帳の読み取り結果の「年」の欄。 */
	get memoYear(): Locator {
		return this.page.getByLabel(ja.photo_page_memo_year_label, { exact: true });
	}

	get reviewButton(): Locator {
		return this.page.getByRole('button', { name: ja.common_review_button, exact: true });
	}

	get backButton(): Locator {
		return this.page.getByRole('button', { name: ja.common_back_button, exact: true });
	}

	/** 「数字で入力する」でホーム画面へ戻るのを待つ。ホーム画面では追加のシートが開く (`HomePage.addSheet`)。 */
	async enterManually(): Promise<HomePage> {
		await this.manualEntryButton.click();
		await this.page.waitForURL('/');
		return new HomePage(this.page);
	}

	/** 戻るボタンでホーム画面へ戻るのを待つ。離れる前の確認が出ない状態で使う。 */
	async back(): Promise<HomePage> {
		await this.backButton.click();
		await this.page.waitForURL('/');
		return new HomePage(this.page);
	}
}
