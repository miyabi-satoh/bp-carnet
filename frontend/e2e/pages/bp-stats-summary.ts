import type { Locator, Page } from '@playwright/test';

/** 朝・夜の平均 (bp-stats-summary.svelte)。ホーム画面・レポート・管理者のユーザー詳細で共通。 */
export class BpStatsSummary {
	constructor(private readonly page: Page) {}

	/** 平均のカード。`label` は `ja.bp_stats_morning_average_label` などの見出し。 */
	averageCard(label: string): Locator {
		return this.page.getByText(label, { exact: true }).locator('..');
	}
}
