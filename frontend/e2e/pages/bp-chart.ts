import type { Locator, Page } from '@playwright/test';
import { ja } from '../helpers';
import { waitForChart } from '../page-waits';

/** 血圧のグラフ (bp-chart.svelte) と系列の表示切り替え。ホーム画面とレポートで共通。 */
export class BpChart {
	constructor(private readonly page: Page) {}

	/** 系列の表示切り替えボタンの並び。記録が2日分に満たないとグラフごと出ない。 */
	get legend(): Locator {
		return this.page.getByRole('group', { name: ja.chart_series_toggle_group_label });
	}

	/** 系列の表示切り替えボタン。`label` は `ja.chart_morning_systolic_label` などの系列名。 */
	seriesToggle(label: string): Locator {
		return this.legend.getByRole('button', { name: label, exact: true });
	}

	/** グラフの描かれる枠。 */
	get area(): Locator {
		return this.page.locator('[data-slot="chart"]');
	}

	/** 描かれた折れ線 (系列ごとに1本)。 */
	get lines(): Locator {
		return this.page.locator('.lc-path');
	}

	/** `count` 本の折れ線が描き終わるまで待つ (→ waitForChart)。 */
	async waitForLines(count: number): Promise<void> {
		await waitForChart(this.page, count);
	}
}
