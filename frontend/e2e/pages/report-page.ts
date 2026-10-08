import type { Locator, Page } from '@playwright/test';
import { ja } from '../helpers';
import { BpChart } from './bp-chart';
import { BpStatsSummary } from './bp-stats-summary';
import { HomePage } from './home-page';
import { PeriodNav } from './period-nav';

/** 印刷用レポート (`/report`)。 */
export class ReportPage {
	readonly periodNav: PeriodNav;
	readonly chart: BpChart;
	readonly stats: BpStatsSummary;

	constructor(private readonly page: Page) {
		this.periodNav = new PeriodNav(page);
		this.chart = new BpChart(page);
		this.stats = new BpStatsSummary(page);
	}

	/** 期間の見出し。`label` は date-helpers.ts の `reportRangeLabel`。 */
	rangeLabel(label: string): Locator {
		return this.page.getByText(label, { exact: true });
	}

	/** 表の行 (見出しの行を含む)。 */
	get tableRows(): Locator {
		return this.page.getByRole('row');
	}

	cell(name: string): Locator {
		return this.page.getByRole('cell', { name });
	}

	columnHeader(name: string): Locator {
		return this.page.getByRole('columnheader', { name, exact: true });
	}

	get printButton(): Locator {
		return this.page.getByRole('button', { name: ja.report_print_button, exact: true });
	}

	get backLink(): Locator {
		return this.page.getByRole('link', { name: ja.common_back_button, exact: true });
	}

	/** 戻るリンクでホーム画面へ戻るのを待つ。 */
	async back(): Promise<HomePage> {
		await this.backLink.click();
		await this.page.waitForURL('/');
		return new HomePage(this.page);
	}
}
