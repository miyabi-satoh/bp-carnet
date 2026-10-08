import { describe, expect, it } from 'vitest';
import { RECORD_LIST_PAGE_SIZE, RecordsViewState } from './records-view.svelte';

describe('RecordsViewState', () => {
	it('一覧に出す件数は期間を変えたときだけ戻し、取り直しただけでは戻さない', async () => {
		const view = new RecordsViewState(async () => ({
			ok: true,
			records: [],
			summary: { days: [] }
		}));
		view.listLimit = RECORD_LIST_PAGE_SIZE * 3;

		await view.load();
		expect(view.listLimit).toBe(RECORD_LIST_PAGE_SIZE * 3);

		view.changePeriod('2026-09-01', '2026-09-30');
		expect(view.listLimit).toBe(RECORD_LIST_PAGE_SIZE);
	});

	it('足した・直した日の記録が「さらに表示」の後ろなら、その日がすべて出るまで出す件数を広げる', async () => {
		// 新しい順に1日1件、250件。150件目と151件目 (添字149・150) を同じ日にする。
		const records = Array.from({ length: 250 }, (_, i) => ({
			id: i + 1,
			localMeasuredAt: `d${String(i === 150 ? 149 : i).padStart(3, '0')}T07:00`,
			systolic: 120,
			diastolic: 80,
			version: 1,
			memo: ''
		}));
		const view = new RecordsViewState(async () => ({ ok: true, records, summary: { days: [] } }));
		await view.load();

		view.revealDate('d010');
		expect(view.listLimit).toBe(RECORD_LIST_PAGE_SIZE);

		view.revealDate('d149');
		expect(view.listLimit).toBe(RECORD_LIST_PAGE_SIZE * 2);

		view.revealDate('d999');
		expect(view.listLimit).toBe(RECORD_LIST_PAGE_SIZE * 2);
	});
});
