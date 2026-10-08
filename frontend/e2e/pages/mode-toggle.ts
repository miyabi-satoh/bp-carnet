import { expect, type Locator, type Page } from '@playwright/test';
import { ja } from '../helpers';

/** テーマの切り替え (mode-toggle.svelte)。押すとライト・ダーク・システムのメニューが開く。 */
export class ModeToggle {
	constructor(private readonly page: Page) {}

	get button(): Locator {
		return this.page.getByRole('button', { name: ja.theme_toggle_label, exact: true });
	}

	/** メニューの選択肢。`label` は `ja.theme_dark` など。 */
	option(label: string): Locator {
		return this.page.getByRole('menuitemradio', { name: label, exact: true });
	}

	get menu(): Locator {
		return this.page.getByRole('menu');
	}

	/** メニューを開いて `label` を選び、メニューが閉じるまで待つ。 */
	async choose(label: string): Promise<void> {
		await this.button.click();
		await this.option(label).click();
		await expect(this.menu).toBeHidden();
	}
}
