import { expect, type Download, type Locator, type Page } from '@playwright/test';
import { chooseFile, ja } from '../helpers';
import { ChangePasswordPage } from './change-password-page';
import { CsvImportPage } from './csv-import-page';
import { DeleteAccountDialog } from './delete-account-dialog';
import { LayoutHeader } from './layout-header';
import { LogoutDialog } from './logout-dialog';
import { TimePicker } from './time-picker';
import { TopupConfirmDialog } from './topup-confirm-dialog';
import { UnlinkIdentityDialog } from './unlink-identity-dialog';

/** 設定画面 (`/settings`)。 */
export class SettingsPage {
	readonly header: LayoutHeader;
	/** 朝の時間帯の終わり。 */
	readonly morningEnd: TimePicker;

	constructor(private readonly page: Page) {
		this.header = new LayoutHeader(page);
		this.morningEnd = new TimePicker(page, ja.settings_morning_end_label);
	}

	async open(): Promise<void> {
		await this.page.goto('/settings');
	}

	/** 「アカウント」の欄。どのユーザーにも出るので、画面が出そろったことの目印にも使う。 */
	get account(): Locator {
		return this.page.getByRole('region', { name: ja.settings_account_section_title });
	}

	/** 「写真の読み取り」の欄。写真を送ることに同意しているときだけ出る。 */
	get ocrSection(): Locator {
		return this.page.getByRole('region', { name: ja.settings_ocr_section_title });
	}

	get withdrawOcrConsentButton(): Locator {
		return this.ocrSection.getByRole('button', {
			name: ja.settings_ocr_consent_withdraw_button,
			exact: true
		});
	}

	/** 「写真の読み取り」の欄の「読み取りを購入...」で確認のダイアログを開く。 */
	async openTopup(): Promise<TopupConfirmDialog> {
		await this.ocrSection
			.getByRole('button', { name: ja.settings_ocr_topup_button, exact: true })
			.click();
		return new TopupConfirmDialog(this.page);
	}

	/** 同意を取り消したときのお知らせ。 */
	get ocrConsentWithdrawnNotice(): Locator {
		return this.page.getByRole('alertdialog', { name: ja.settings_ocr_consent_withdrawn_title });
	}

	/** 「このアプリについて」の欄。 */
	get about(): Locator {
		return this.page.getByRole('region', { name: ja.settings_about_section_title });
	}

	/** 「このアプリについて」の欄のリンクすべて。 */
	get aboutLinks(): Locator {
		return this.about.getByRole('link');
	}

	/** 「このアプリについて」の欄の、読むものや問い合わせ先へのリンク。 */
	aboutLink(name: string): Locator {
		return this.about.getByRole('link', { name });
	}

	/** CSV の取り込みを終えて戻ったときのお知らせ。 */
	get importedNotice(): Locator {
		return this.page.getByRole('alertdialog', { name: ja.settings_import_done_title });
	}

	/** ブラウザで CSV を書き出したときのお知らせ。 */
	get exportedNotice(): Locator {
		return this.page.getByRole('alertdialog', { name: ja.settings_export_done_title });
	}

	/** CSV を書き出して保存されたファイルを受け取り、書き出したお知らせを閉じる。 */
	async exportCsv(): Promise<Download> {
		const download = this.page.waitForEvent('download');
		await this.exportCsvButton.click();
		const file = await download;
		await expect(this.exportedNotice).toContainText(file.suggestedFilename());
		await this.exportedNotice
			.getByRole('button', { name: ja.common_close_button, exact: true })
			.click();
		return file;
	}

	/** アカウントの欄に出るユーザー ID。 */
	username(name: string): Locator {
		return this.page.getByText(name, { exact: true });
	}

	/** 欄の見出しの横に出る「保存しました」。`sectionTitle` は欄の見出し。 */
	savedNotice(sectionTitle: string): Locator {
		return this.page
			.getByRole('region', { name: sectionTitle })
			.getByText(ja.settings_saved_badge, { exact: true });
	}

	get timeZone(): Locator {
		return this.page.getByRole('combobox', {
			name: ja.settings_timezone_section_title,
			exact: true
		});
	}

	/** タイムゾーンの選択肢の先頭。自動設定のときは、ブラウザのタイムゾーンがここに出る。 */
	get autoTimeZoneOption(): Locator {
		return this.timeZone.locator('option').first();
	}

	get exportCsvButton(): Locator {
		return this.page.getByRole('button', { name: ja.settings_export_csv_button, exact: true });
	}

	get importButton(): Locator {
		return this.page.getByRole('button', { name: ja.settings_import_button, exact: true });
	}

	/** 「CSVファイルを読み込む...」で `file` を選び、取り込みのページへ移るのを待つ。 */
	async pickCsv(file: Parameters<typeof chooseFile>[2]): Promise<CsvImportPage> {
		await chooseFile(this.page, this.importButton, file);
		await this.page.waitForURL('/settings/import');
		return new CsvImportPage(this.page);
	}

	async openChangePassword(): Promise<ChangePasswordPage> {
		await this.page.getByRole('link', { name: ja.settings_change_password_button }).click();
		return new ChangePasswordPage(this.page);
	}

	/** 画面の中の「ログアウト」から確認を開く。 */
	async openLogout(): Promise<LogoutDialog> {
		await this.page
			.getByRole('button', { name: ja.common_logout_button, exact: true })
			.first()
			.click();
		return new LogoutDialog(this.page);
	}

	/** 削除を予約した後に出る、取り消し方の案内 (本人が起点の予約だけ)。 */
	get deletionCancelHint(): Locator {
		return this.page.getByText(ja.settings_deletion_cancel_hint, { exact: true });
	}

	get deleteAccountButton(): Locator {
		return this.page.getByRole('button', { name: ja.common_delete_account_button, exact: true });
	}

	async openDeleteAccount(): Promise<DeleteAccountDialog> {
		await this.deleteAccountButton.click();
		return new DeleteAccountDialog(this.page);
	}

	/** ページの見出し「設定」。 */
	get heading(): Locator {
		return this.page.getByRole('heading', { level: 1, name: ja.settings_title, exact: true });
	}

	/** 「ログイン方法」の欄の見出し。 */
	get loginMethodsHeading(): Locator {
		return this.loginMethods.getByRole('heading', {
			name: ja.settings_login_methods_section_title,
			exact: true
		});
	}

	/** 「ログイン方法」の欄。連携している方法が無いときは出ない。 */
	get loginMethods(): Locator {
		return this.page.getByRole('region', { name: ja.settings_login_methods_section_title });
	}

	/** 連携の解除を開くボタン。`providerLabel` は「LINE」「Google」。 */
	unlinkButton(providerLabel: string): Locator {
		return this.loginMethods.getByRole('button', {
			name: `${providerLabel}: ${ja.settings_unlink_button}`,
			exact: true
		});
	}

	/** 最後のログイン方法に出る、解除できない理由。 */
	get lastMethodNotes(): Locator {
		return this.loginMethods.getByText(ja.settings_unlink_last_method_note, { exact: true });
	}

	async openUnlink(providerLabel: string): Promise<UnlinkIdentityDialog> {
		await this.unlinkButton(providerLabel).click();
		return new UnlinkIdentityDialog(this.page, providerLabel);
	}
}
