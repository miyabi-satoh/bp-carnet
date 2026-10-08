import type { Locator, Page } from '@playwright/test';
import { ja } from '../helpers';

/** 時刻の選択 (time-picker.svelte)。押すと時・分のボタンが並ぶポップオーバーが開く。 */
export class TimePicker {
	constructor(
		private readonly page: Page,
		private readonly label: string
	) {}

	/** 開くボタン。欄の名前は見えない文言としてボタンに入り、見える文字は時刻だけ。 */
	get trigger(): Locator {
		return this.page.getByRole('button', { name: this.label });
	}

	get hours(): Locator {
		return this.page.getByRole('group', { name: ja.time_picker_hour_label, exact: true });
	}

	get minutes(): Locator {
		return this.page.getByRole('group', { name: ja.time_picker_minute_label, exact: true });
	}

	hourButton(hour: number): Locator {
		return this.hours.getByRole('button', { name: String(hour), exact: true });
	}

	get closeButton(): Locator {
		return this.page.getByRole('button', { name: ja.common_close_button, exact: true });
	}
}
