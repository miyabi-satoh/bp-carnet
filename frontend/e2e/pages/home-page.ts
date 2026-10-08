import { expect, type Locator, type Page } from '@playwright/test';
import { chooseFile, ja } from '../helpers';
import { BpChart } from './bp-chart';
import { BpStatsSummary } from './bp-stats-summary';
import { LayoutHeader } from './layout-header';
import { PeriodNav } from './period-nav';
import { PhotoPage } from './photo-page';
import { RecordCard } from './record-card';
import { RecordDeleteDialog } from './record-delete-dialog';
import { RecordFormSheet } from './record-form-sheet';
import { ReportPage } from './report-page';

/** ホーム画面 (`/`)。 */
export class HomePage {
	readonly header: LayoutHeader;
	readonly periodNav: PeriodNav;
	readonly chart: BpChart;
	readonly stats: BpStatsSummary;

	constructor(private readonly page: Page) {
		this.header = new LayoutHeader(page);
		this.periodNav = new PeriodNav(page);
		this.chart = new BpChart(page);
		this.stats = new BpStatsSummary(page);
	}

	/** 開き、記録一覧の見出しが出るまで待つ。`+layout.ts` のログイン確認を挟むため、`goto` だけでは中身が出そろわない。 */
	async open(): Promise<void> {
		await this.page.goto('/');
		await expect(this.heading).toBeVisible();
	}

	/** 記録一覧の見出し。出ていればホーム画面が出そろっている。 */
	get heading(): Locator {
		return this.page.getByRole('heading', { name: ja.home_history_title });
	}

	get addRecordButton(): Locator {
		return this.page.getByRole('button', { name: ja.home_add_record_button, exact: true });
	}

	/** 「写真で記録...」。読み取りが無効 (GEMINI_API_KEY が無い) だと出ない。 */
	get photoButton(): Locator {
		return this.page.getByRole('button', { name: ja.home_photo_ocr_button, exact: true });
	}

	/** 期間内に記録が無いときの文。 */
	get periodEmptyMessage(): Locator {
		return this.page.getByText(ja.record_empty, { exact: true });
	}

	/** 記録が1件も無いときの、下のボタンから記録できるという案内。読み取りの有無で文が変わるので、最初のボタン名の前までで掴む。 */
	get firstRecordHint(): Locator {
		const [prefix] = ja.home_first_record_hint.split('{manual}');
		return this.page.getByText(prefix);
	}

	get reportLink(): Locator {
		return this.page.getByRole('link', { name: ja.home_report_button, exact: true });
	}

	/** ログインしたときに、アカウントの削除の予約を取り消したと知らせるお知らせ。 */
	get deletionCancelledNotice(): Locator {
		return this.page.getByRole('alertdialog', { name: ja.account_deletion_cancelled_title });
	}

	/** 「印刷用レポート」からレポートへ移る。 */
	async openReport(): Promise<ReportPage> {
		await this.reportLink.click();
		await this.page.waitForURL((url) => url.pathname === '/report');
		return new ReportPage(this.page);
	}

	/** 追加のシート。開くのは `openAddSheet()` か、写真で記録のページの「数字で入力する」。 */
	get addSheet(): RecordFormSheet {
		return new RecordFormSheet(this.page, 'add');
	}

	/** 「手動で入力...」から追加のシートを開く。 */
	async openAddSheet(): Promise<RecordFormSheet> {
		await this.addRecordButton.click();
		return this.addSheet;
	}

	/** 「写真で記録...」で `file` を選び、写真で記録のページへ移って読み取る。 */
	async pickPhoto(file: Parameters<typeof chooseFile>[2]): Promise<PhotoPage> {
		const photoPage = await this.choosePhoto(file);
		await photoPage.readButton.click();
		return photoPage;
	}

	/** 「写真で記録...」で `file` を選び、写真で記録のページへ移るのを待つ。読み取りはまだ始まらない。 */
	async choosePhoto(file: Parameters<typeof chooseFile>[2]): Promise<PhotoPage> {
		await chooseFile(this.page, this.photoButton, file);
		await this.page.waitForURL('/record/photo');
		return new PhotoPage(this.page);
	}

	/** 記録カード。`text` (メモなど) とボタンを両方含む要素のうち、ボタンを全部含む一番内側のもの (祖先ほど前に並ぶ)。 */
	recordCard(text: string): RecordCard {
		return new RecordCard(
			this.page
				.locator('div')
				.filter({ has: this.page.getByText(text, { exact: true }) })
				.filter({ has: this.page.getByRole('button', { name: RecordCard.EDIT_LABEL }) })
				.filter({ has: this.page.getByRole('button', { name: RecordCard.DELETE_LABEL }) })
				.last()
		);
	}

	/** `text` の記録カードから直すシートを開く。 */
	async openRecord(text: string): Promise<RecordFormSheet> {
		await this.recordCard(text).editButton.click();
		return new RecordFormSheet(this.page, 'edit');
	}

	/** `text` の記録カードのゴミ箱から、削除の確認を開く。 */
	async openDelete(text: string): Promise<RecordDeleteDialog> {
		await this.recordCard(text).deleteButton.click();
		return new RecordDeleteDialog(this.page);
	}
}
