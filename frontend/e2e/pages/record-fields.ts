import type { Locator, Page } from '@playwright/test';
import { ja, requiredLabel } from '../helpers';

/** 記録の入力欄 (record-fields.svelte)。記録フォームのシートと、写真で記録のページのフォームで共通。 */
export class RecordFields {
	constructor(private readonly scope: Page | Locator) {}

	get systolic(): Locator {
		return this.scope.getByLabel(requiredLabel(ja.record_systolic_label), { exact: true });
	}

	get diastolic(): Locator {
		return this.scope.getByLabel(requiredLabel(ja.record_diastolic_label), { exact: true });
	}

	get pulse(): Locator {
		return this.scope.getByLabel(ja.record_pulse_label, { exact: true });
	}

	get memo(): Locator {
		return this.scope.getByLabel(ja.record_memo_label, { exact: true });
	}

	/** メモ欄の下の候補のチップ。 */
	get memoSuggestions(): Locator {
		return this.scope
			.getByRole('group', { name: ja.record_memo_suggestions_label, exact: true })
			.getByRole('button');
	}

	/** 候補のチップが、メモ欄の幅に収まっているか (右端がはみ出していないか)。 */
	async memoSuggestionFitsInWidth(chip: Locator): Promise<boolean> {
		const [chipBox, memoBox] = await Promise.all([chip.boundingBox(), this.memo.boundingBox()]);
		if (chipBox === null || memoBox === null)
			throw new Error('候補のチップかメモ欄が見えていません');
		// 小数の丸めで 1px 未満ずれることがある。
		return chipBox.x + chipBox.width <= memoBox.x + memoBox.width + 1;
	}

	/** 候補のチップの文言が「…」で省かれているか (文言がチップの中に収まりきっていないか)。 */
	async isMemoSuggestionTruncated(chip: Locator): Promise<boolean> {
		return chip.locator('span').evaluate((el) => el.scrollWidth > el.clientWidth);
	}

	get date(): Locator {
		return this.scope.getByLabel(requiredLabel(ja.record_measured_at_label), { exact: true });
	}

	get time(): Locator {
		return this.scope.getByLabel(ja.record_time_label, { exact: true });
	}

	/** 渡した欄だけを埋める。 */
	async fill(values: {
		systolic?: string;
		diastolic?: string;
		pulse?: string;
		memo?: string;
	}): Promise<void> {
		if (values.systolic !== undefined) await this.systolic.fill(values.systolic);
		if (values.diastolic !== undefined) await this.diastolic.fill(values.diastolic);
		if (values.pulse !== undefined) await this.pulse.fill(values.pulse);
		if (values.memo !== undefined) await this.memo.fill(values.memo);
	}
}
