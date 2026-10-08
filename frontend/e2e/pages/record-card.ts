import type { Locator } from '@playwright/test';
import { ja } from '../helpers';

/** 記録カード (record-card.svelte)。ホームと管理者の閲覧に出る。 */
export class RecordCard {
	/** ボタンの読み上げ (「{measuredAt} の記録を直す」など) の、日時の後ろ。 */
	static readonly EDIT_LABEL = new RegExp(
		`${ja.record_card_edit_label.replace('{measuredAt}', '')}$`
	);
	static readonly DELETE_LABEL = new RegExp(
		`${ja.record_card_delete_label.replace('{measuredAt}', '')}$`
	);

	constructor(readonly root: Locator) {}

	get editButton(): Locator {
		return this.root.getByRole('button', { name: RecordCard.EDIT_LABEL });
	}

	/** ゴミ箱。 */
	get deleteButton(): Locator {
		return this.root.getByRole('button', { name: RecordCard.DELETE_LABEL });
	}
}
