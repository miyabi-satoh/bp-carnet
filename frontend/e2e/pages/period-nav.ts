import type { Locator, Page } from '@playwright/test';
import { ja } from '../helpers';

/** 期間の切り替え (period-nav.svelte)。ホーム画面とレポートで共通。 */
export class PeriodNav {
	constructor(private readonly page: Page) {}

	private button(name: string): Locator {
		return this.page.getByRole('button', { name, exact: true });
	}

	get weekButton(): Locator {
		return this.button(ja.period_nav_week_button);
	}

	get monthButton(): Locator {
		return this.button(ja.period_nav_month_button);
	}

	get prevButton(): Locator {
		return this.button(ja.period_nav_prev_button);
	}

	get nextButton(): Locator {
		return this.button(ja.period_nav_next_button);
	}

	/** 週の表示中の期間のラベル。押すと今週へ戻る。 */
	get thisWeekButton(): Locator {
		return this.jumpButton(ja.period_nav_this_week_button);
	}

	/** 月の表示中の期間のラベル。押すと今月へ戻る。 */
	get thisMonthButton(): Locator {
		return this.jumpButton(ja.period_nav_this_month_button);
	}

	/** 名前は「見えている期間、行き先」(例: `9月7日〜13日、今週へ`)。期間の部分は問わない。 */
	private jumpButton(message: string): Locator {
		const suffix = message.replace('{period}', '');
		return this.page.getByRole('button', { name: new RegExp(`${suffix}$`) });
	}

	/** 期間を指定しているときのラベル。`label` は date-helpers.ts の `rangeLabel`。 */
	rangeButton(label: string): Locator {
		return this.button(label);
	}

	get filterClearButton(): Locator {
		return this.button(ja.period_nav_filter_clear_button);
	}

	/** 「期間を指定」を開き、`from`〜`to` (`YYYY-MM-DD`) で絞り込む。開いていないときに呼ぶ。 */
	async filter(from: string, to: string): Promise<void> {
		await this.page.getByText(ja.period_nav_filter_toggle_label, { exact: true }).click();
		await this.applyFilter(from, to);
	}

	/** 開いている「期間を指定」に入れ直して絞り込む。 */
	async applyFilter(from: string, to: string): Promise<void> {
		await this.page.getByLabel(ja.period_nav_filter_from_label, { exact: true }).fill(from);
		await this.filterTo.fill(to);
		await this.button(ja.period_nav_filter_apply_button).click();
	}

	get filterTo(): Locator {
		return this.page.getByLabel(ja.period_nav_filter_to_label, { exact: true });
	}

	/** 開始日が終了日より後のときに、欄の下に出るエラー。 */
	get filterRangeError(): Locator {
		return this.page.getByText(ja.period_nav_filter_range_error, { exact: true });
	}
}
