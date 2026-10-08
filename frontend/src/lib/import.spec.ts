import { describe, expect, it, vi, beforeEach } from 'vitest';
import {
	buildImportDiff,
	decodeCsvText,
	deletionWarning,
	fetchExistingEntries,
	importDateRange,
	importRows,
	applyEditedRow,
	importRowStatus,
	initialKeep,
	parseCsvImport,
	fillPlaceholderTimes,
	revalidateRow,
	rowToRequest,
	type ImportDiffGroup,
	type ImportDiffStatus,
	type ImportRow,
	type ImportRowStatus
} from './import';
import { splitMeasuredAt } from './record-form';
import { DEFAULT_PERIOD_THRESHOLDS } from '$lib/settings';
import type { DayPeriod } from '$lib/period';
import * as m from '$lib/paraglide/messages.js';

vi.mock('$lib/api/client', () => ({
	client: { POST: vi.fn(), GET: vi.fn() }
}));
import { client } from '$lib/api/client';

const postMock = client.POST as unknown as ReturnType<typeof vi.fn>;
const getMock = client.GET as unknown as ReturnType<typeof vi.fn>;

// エクスポート (`GET /records/export`) が実際に生成する形式そのもの
// (BOM・ヘッダー行・CRLF改行・`memo` のカンマ/ダブルクォート/改行エスケープ)。
const EXPORTER_CSV =
	'﻿' +
	'measuredAt,systolic,diastolic,pulse,memo\r\n' +
	'2026-09-01 08:30,120,80,65,"朝,晩""服薬""\n異常なし"\r\n' +
	'2026-09-05 08:30,118,78,,\r\n';

const TIME_ZONE = 'Asia/Tokyo';

describe('parseCsvImport', () => {
	it('parses the exporter output (BOM, CRLF, quoted memo)', () => {
		const result = parseCsvImport(EXPORTER_CSV, TIME_ZONE, DEFAULT_PERIOD_THRESHOLDS);
		if ('error' in result) throw new Error(`unexpected error: ${result.error}`);

		expect(result.rows).toHaveLength(2);
		expect(result.rows[0].rowError).toBeUndefined();
		expect(result.rows[0].systolic).toBe('120');
		expect(result.rows[0].diastolic).toBe('80');
		expect(result.rows[0].pulse).toBe('65');
		expect(result.rows[0].memo).toBe('朝,晩"服薬"\n異常なし');

		expect(result.rows[1].rowError).toBeUndefined();
		expect(result.rows[1].pulse).toBe('');
		expect(result.rows[1].memo).toBe('');
	});

	it('ignores unknown columns (e.g. id/createdAt from older exports)', () => {
		const csv =
			'id,measuredAt,systolic,diastolic,pulse,memo,createdAt\n' +
			'1,2026-09-01T08:30:00Z,120,80,65,朝,2026-09-01T08:30:01.000Z\n';
		const result = parseCsvImport(csv, TIME_ZONE, DEFAULT_PERIOD_THRESHOLDS);
		if ('error' in result) throw new Error(`unexpected error: ${result.error}`);
		expect(result.rows[0].rowError).toBeUndefined();
		expect(result.rows[0].memo).toBe('朝');
	});

	it('errors when a required header is missing', () => {
		const result = parseCsvImport(
			'measuredAt,systolic\n2026-09-01T08:30:00Z,120\n',
			TIME_ZONE,
			DEFAULT_PERIOD_THRESHOLDS
		);
		expect(result).toEqual({ error: m.import_error_csv_missing_columns({ columns: 'diastolic' }) });
	});

	it('maps columns by name regardless of order', () => {
		const csv = 'diastolic,measuredAt,systolic\n80,2026-09-01T08:30:00Z,120\n';
		const result = parseCsvImport(csv, TIME_ZONE, DEFAULT_PERIOD_THRESHOLDS);
		if ('error' in result) throw new Error(`unexpected error: ${result.error}`);
		expect(result.rows[0].systolic).toBe('120');
		expect(result.rows[0].diastolic).toBe('80');
	});

	it('flags only the row with a non-numeric value, others stay unaffected', () => {
		const csv =
			'measuredAt,systolic,diastolic\n' +
			'2026-09-01T08:30:00Z,not-a-number,80\n' +
			'2026-09-02T08:30:00Z,120,80\n';
		const result = parseCsvImport(csv, TIME_ZONE, DEFAULT_PERIOD_THRESHOLDS);
		if ('error' in result) throw new Error(`unexpected error: ${result.error}`);
		expect(result.rows[0].rowError).toContain(m.record_systolic_label());
		expect(result.rows[1].rowError).toBeUndefined();
	});

	it('returns no rows for a header-only CSV', () => {
		const result = parseCsvImport(
			'measuredAt,systolic,diastolic\n',
			TIME_ZONE,
			DEFAULT_PERIOD_THRESHOLDS
		);
		if ('error' in result) throw new Error(`unexpected error: ${result.error}`);
		expect(result.rows).toHaveLength(0);
	});
});

describe('applyEditedRow', () => {
	function row(overrides: Partial<ImportRow> = {}): ImportRow {
		return {
			keep: false,
			measuredOnDate: '',
			time: '',
			timeIsPlaceholder: true,
			systolic: 'abc',
			diastolic: '80',
			pulse: '',
			memo: '',
			rowError: 'x',
			...overrides
		};
	}

	it('シートで直した値を書き戻し、チェックを入れて仮の時刻を解く', () => {
		const target = row();
		applyEditedRow(target, {
			localMeasuredAt: '2026-09-01T07:15',
			systolic: 120,
			diastolic: 80,
			pulse: 70,
			memo: 'めも'
		});
		expect(target).toMatchObject({
			keep: true,
			measuredOnDate: '2026-09-01',
			time: '07:15',
			timeIsPlaceholder: false,
			systolic: '120',
			diastolic: '80',
			pulse: '70',
			memo: 'めも'
		});
		expect(target.rowError).toBeUndefined();
	});

	// 確かめて更新した行は取り込む。読み込んだ直後にアプリが外した行を、本人の確認で戻す道。
	it('エラーの無かった行でも、更新すればチェックが入る', () => {
		const target = row({ rowError: undefined, systolic: '120', keep: false });
		applyEditedRow(target, {
			localMeasuredAt: '2026-09-01T07:15',
			systolic: 120,
			diastolic: 80,
			memo: ''
		});
		expect(target.keep).toBe(true);
	});

	it('脈拍を消した行は、空文字に戻す', () => {
		const target = row();
		applyEditedRow(target, {
			localMeasuredAt: '2026-09-01T07:15',
			systolic: 120,
			diastolic: 80,
			memo: ''
		});
		expect(target.pulse).toBe('');
	});
});

describe('importRowStatus / initialKeep', () => {
	function row(overrides: Partial<ImportRow>): ImportRow {
		return {
			keep: true,
			...splitMeasuredAt('2026-09-01T07:00'),
			systolic: '120',
			diastolic: '80',
			pulse: '',
			memo: '',
			...overrides
		};
	}

	it('直さないと取り込めない行だけをエラーにする', () => {
		const cases: [Partial<ImportRow>, ImportRowStatus][] = [
			[{}, 'ok'],
			[{ rowError: 'x' }, 'error'],
			// 時刻を書かない使い方が普通なので、仮の時刻を振っただけでは知らせない
			// (「朝」「夜」と出して伝える。docs/ocr.md)。
			[{ timeIsPlaceholder: true }, 'ok']
		];
		for (const [overrides, expected] of cases) {
			expect(importRowStatus(row(overrides))).toBe(expected);
		}
	});

	it('読み込んだ直後のチェックは、確かめてほしい行だけ外す', () => {
		expect(initialKeep('ok')).toBe(true);
		expect(initialKeep('warn')).toBe(false);
		expect(initialKeep('error')).toBe(false);
	});
});

describe('revalidateRow', () => {
	function baseRow(): ImportRow {
		return {
			keep: true,
			...splitMeasuredAt('2026-09-01T08:30'),
			systolic: '300',
			diastolic: '80',
			pulse: '',
			memo: ''
		};
	}

	it('reads the initial check state from the row status', () => {
		const result = parseCsvImport(
			'measuredAt,systolic,diastolic\n2026-09-01T07:00,120,80\n2026-09-01T08:00,300,80\n',
			TIME_ZONE,
			DEFAULT_PERIOD_THRESHOLDS
		);
		if ('error' in result) throw new Error(`unexpected error: ${result.error}`);
		expect(result.rows.map((row) => row.keep)).toEqual([true, false]);
	});

	it('sets rowError for out-of-range values', () => {
		const row = baseRow();
		revalidateRow(row);
		expect(row.rowError).toContain(m.record_systolic_label());
	});

	it('keeps non-integer text as typed and reports it instead of treating it as empty', () => {
		const row = { ...baseRow(), systolic: '12a' };
		revalidateRow(row);
		expect(row.systolic).toBe('12a');
		expect(row.rowError).toBe(m.record_error_not_integer({ field: m.record_systolic_label() }));
	});

	it('clears rowError once the value is fixed', () => {
		const row = baseRow();
		revalidateRow(row);
		row.systolic = '120';
		revalidateRow(row);
		expect(row.rowError).toBeUndefined();
	});
});

describe('parseCsvImport measuredAt', () => {
	it('reads the exporter output back as the user timezone', () => {
		const result = parseCsvImport(EXPORTER_CSV, TIME_ZONE, DEFAULT_PERIOD_THRESHOLDS);
		if ('error' in result) throw new Error(`unexpected error: ${result.error}`);
		expect(measuredAt(result.rows[0])).toBe('2026-09-01T08:30');
		expect(rowToRequest(result.rows[0]).localMeasuredAt).toBe('2026-09-01T08:30');
	});

	// 以前の書き出しは UTC (`2026-09-01T08:30:00Z`) だったので、その CSV も読めるようにしておく。
	it('converts an offset datetime into the user timezone', () => {
		const result = parseCsvImport(
			'measuredAt,systolic,diastolic\n2026-09-01T08:30:00Z,120,80\n',
			TIME_ZONE,
			DEFAULT_PERIOD_THRESHOLDS
		);
		if ('error' in result) throw new Error(`unexpected error: ${result.error}`);
		// 08:30 UTC = 17:30 JST。
		expect(measuredAt(result.rows[0])).toBe('2026-09-01T17:30');
		expect(rowToRequest(result.rows[0]).localMeasuredAt).toBe('2026-09-01T17:30');
	});

	it('treats a datetime without an offset as the user timezone', () => {
		const result = parseCsvImport(
			'measuredAt,systolic,diastolic\n2026-09-01 07:15,120,80\n',
			TIME_ZONE,
			DEFAULT_PERIOD_THRESHOLDS
		);
		if ('error' in result) throw new Error(`unexpected error: ${result.error}`);
		expect(measuredAt(result.rows[0])).toBe('2026-09-01T07:15');
	});

	// 日本語の Excel で保存し直した CSV の形 (docs/import-export.md)。
	it('reads the Excel style datetime and date with slashes and unpadded numbers', () => {
		const result = parseCsvImport(
			'measuredAt,systolic,diastolic\n2026/9/1 7:10,120,80\n2026/9/2,118,78\n2026/9-3 7:10,119,79\n',
			TIME_ZONE,
			DEFAULT_PERIOD_THRESHOLDS
		);
		if ('error' in result) throw new Error(`unexpected error: ${result.error}`);
		expect(measuredAt(result.rows[0])).toBe('2026-09-01T07:10');
		expect(result.rows[1].measuredOnDate).toBe('2026-09-02');
		expect(result.rows[1].timeIsPlaceholder).toBe(true);
		// 区切りが混ざっていれば読まない。
		expect(result.rows[2].rowError).toContain(m.record_measured_at_label());
	});

	it('flags a datetime that does not exist', () => {
		const result = parseCsvImport(
			'measuredAt,systolic,diastolic\n2026-02-30T07:15,120,80\n',
			TIME_ZONE,
			DEFAULT_PERIOD_THRESHOLDS
		);
		if ('error' in result) throw new Error(`unexpected error: ${result.error}`);
		expect(result.rows[0].rowError).toContain(m.record_measured_at_label());
	});

	// 時刻を書かない使い方も普通なので受け付け、手書きメモの読み取りと同じ規則で仮時刻を振る (docs/import-export.md)。
	it('accepts a date without a time and gives it a placeholder time in the morning/evening periods', () => {
		const result = parseCsvImport(
			'measuredAt,systolic,diastolic\n' +
				'2026-09-01,120,80\n' +
				'2026-09-01,124,82\n' +
				'2026-09-02,118,78\n' +
				'2026-09-02T07:15,119,79\n',
			TIME_ZONE,
			DEFAULT_PERIOD_THRESHOLDS
		);
		if ('error' in result) throw new Error(`unexpected error: ${result.error}`);
		expect(result.rows.map((row) => [measuredAt(row), row.timeIsPlaceholder, row.keep])).toEqual([
			['2026-09-01T07:00', true, true],
			['2026-09-01T21:00', true, true],
			['2026-09-02T07:00', true, true],
			['2026-09-02T07:15', false, true]
		]);
		expect(result.rows.every((row) => row.rowError === undefined)).toBe(true);
	});

	it('flags a date without a time that does not exist', () => {
		const result = parseCsvImport(
			'measuredAt,systolic,diastolic\n2026-02-30,120,80\n',
			TIME_ZONE,
			DEFAULT_PERIOD_THRESHOLDS
		);
		if ('error' in result) throw new Error(`unexpected error: ${result.error}`);
		expect(result.rows[0].rowError).toContain(m.record_measured_at_label());
		expect(result.rows[0].timeIsPlaceholder).toBe(false);
		expect(result.rows[0].keep).toBe(false);
	});

	it('flags a datetime with non-zero seconds instead of silently dropping them', () => {
		const result = parseCsvImport(
			'measuredAt,systolic,diastolic\n' +
				'2026-09-01T08:30:45Z,120,80\n' +
				'2026-09-01 07:15:99,120,80\n' +
				'2026-09-01 07:15:00,120,80\n',
			TIME_ZONE,
			DEFAULT_PERIOD_THRESHOLDS
		);
		if ('error' in result) throw new Error(`unexpected error: ${result.error}`);
		expect(result.rows[0].rowError).toContain(m.record_measured_at_label());
		expect(result.rows[1].rowError).toContain(m.record_measured_at_label());
		expect(measuredAt(result.rows[2])).toBe('2026-09-01T07:15');
	});
});

describe('decodeCsvText', () => {
	it('UTF-8 として読めなければ Shift_JIS として読む', () => {
		const utf8 = new TextEncoder().encode('起床後');
		expect(decodeCsvText(utf8.buffer)).toBe('起床後');
		// 「起床後」の Shift_JIS。
		const shiftJis = new Uint8Array([0x8b, 0x4e, 0x8f, 0xb0, 0x8c, 0xe3]);
		expect(decodeCsvText(shiftJis.buffer)).toBe('起床後');
	});
});

describe('fillPlaceholderTimes', () => {
	/** 時刻の無い行。`periodHint` は行に書かれていた朝・夜。 */
	const untimed = (measuredOnDate: string, periodHint?: DayPeriod): ImportRow => ({
		keep: true,
		measuredOnDate,
		time: '',
		timeIsPlaceholder: true,
		periodHint,
		systolic: '120',
		diastolic: '80',
		pulse: '',
		memo: ''
	});
	const timesOf = (rows: ImportRow[], periods = DEFAULT_PERIOD_THRESHOLDS) => {
		fillPlaceholderTimes(rows, periods);
		return rows.map((row) => row.time);
	};

	it('puts a single untimed row in the middle of the morning period', () => {
		expect(timesOf([untimed('2026-09-01')])).toEqual(['07:00']);
	});

	it('splits untimed rows of a day into morning and evening, giving morning the extra row when odd', () => {
		const rows = [untimed('2026-09-01'), untimed('2026-09-01'), untimed('2026-09-01')];
		expect(timesOf(rows)).toEqual(['07:00', '07:01', '21:00']);
	});

	it('counts each day separately and skips rows without a date', () => {
		const rows = [untimed('2026-09-01'), untimed('2026-09-02'), untimed(''), untimed('2026-09-01')];
		expect(timesOf(rows)).toEqual(['07:00', '07:00', '', '21:00']);
	});

	it('leaves rows that already have a time alone', () => {
		const rows = [{ ...untimed('2026-09-01'), time: '08:15', timeIsPlaceholder: false }];
		expect(timesOf(rows)).toEqual(['08:15']);
	});

	// 手帳に「夜」とだけ書いた日の1件を、朝に振らない (並び順の規則より書かれた朝・夜を優先する)。
	it('puts rows in the period written beside them', () => {
		expect(timesOf([untimed('2026-09-01', 'evening')])).toEqual(['21:00']);
		const rows = [untimed('2026-09-01', 'evening'), untimed('2026-09-01', 'morning')];
		expect(timesOf(rows)).toEqual(['21:00', '07:00']);
	});

	it('keeps the order within a period when written and unwritten rows share it', () => {
		const rows = [
			untimed('2026-09-01', 'morning'),
			untimed('2026-09-01', 'morning'),
			untimed('2026-09-01')
		];
		// 書かれていない3件目は、並び順で夜 (3件のうち後半)。
		expect(timesOf(rows)).toEqual(['07:00', '07:01', '21:00']);
	});

	// 1日2回で片方に「夜」とあれば、もう片方は朝 (書かれた行が枠を先に使う)。
	it('fills the slots left by written rows with unwritten rows', () => {
		expect(timesOf([untimed('2026-09-01', 'evening'), untimed('2026-09-01')])).toEqual([
			'21:00',
			'07:00'
		]);
		expect(timesOf([untimed('2026-09-01'), untimed('2026-09-01', 'morning')])).toEqual([
			'21:00',
			'07:00'
		]);
	});

	it("follows the user's own morning/evening periods", () => {
		const periods = {
			morningStartMin: 300,
			morningEndMin: 420,
			eveningStartMin: 1140,
			eveningEndMin: 1260
		};
		expect(timesOf([untimed('2026-09-01'), untimed('2026-09-01')], periods)).toEqual([
			'06:00',
			'20:00'
		]);
	});

	// 時間帯が極端に短くても、ずらした結果が時間帯の外に出ないようにする。
	it('stays inside a very short period', () => {
		const periods = { ...DEFAULT_PERIOD_THRESHOLDS, morningStartMin: 600, morningEndMin: 601 };
		const rows = [untimed('2026-09-01'), untimed('2026-09-01'), untimed('2026-09-01')];
		expect(timesOf(rows, periods)).toEqual(['10:00', '10:00', '21:00']);
	});
});

/** 日時は `ImportRow` の2欄ではなく、読みやすさのため連結した形で渡す。 */
function validRow(overrides: Partial<ImportRow> & { measuredAtLocal?: string } = {}): ImportRow {
	const { measuredAtLocal = '2026-09-01T07:00', ...rest } = overrides;
	return {
		keep: true,
		...splitMeasuredAt(measuredAtLocal),
		systolic: '120',
		diastolic: '80',
		pulse: '',
		memo: '',
		...rest
	};
}

/** 日時の2欄を、比較しやすいように連結して取り出す。 */
function measuredAt(row: ImportRow): string {
	return `${row.measuredOnDate}T${row.time}`;
}

describe('importDateRange', () => {
	it('returns null when there are no included rows', () => {
		expect(importDateRange([])).toBeNull();
		expect(importDateRange([validRow({ rowError: 'x' })])).toBeNull();
	});

	it('spans the min/max local date of included rows only', () => {
		const rows = [
			validRow({ measuredAtLocal: '2026-09-05T07:00' }),
			validRow({ measuredAtLocal: '2026-09-01T21:00' }),
			validRow({ measuredAtLocal: '2026-09-10T07:00', rowError: 'x' })
		];
		expect(importDateRange(rows)).toEqual({ from: '2026-09-01', to: '2026-09-05' });
	});
});

describe('buildImportDiff', () => {
	it('classifies exact matches as unchanged, others as added/removed', () => {
		const rows = [
			validRow({
				measuredAtLocal: '2026-09-01T07:00',
				systolic: '126',
				diastolic: '76',
				pulse: '70'
			}),
			validRow({
				measuredAtLocal: '2026-09-05T07:00',
				systolic: '129',
				diastolic: '79',
				pulse: '67'
			})
		];
		const existing = [
			{
				measuredAtLocal: '2026-09-01T07:00',
				systolic: 126,
				diastolic: 76,
				pulse: 70,
				memo: ''
			},
			// 対象行と同じ日にあって対象行と一致しないので、置き換えで消える。
			{
				measuredAtLocal: '2026-09-01T21:20',
				systolic: 130,
				diastolic: 82,
				pulse: 66,
				memo: ''
			}
		];

		const groups = buildImportDiff(rows, existing);
		expect(groups.map((g) => g.date)).toEqual(['2026-09-01', '2026-09-05']);

		expect(groups[0].hasRemoval).toBe(true);
		expect(groups[0].rows).toEqual([
			{ ...existing[0], status: 'unchanged' },
			{ ...existing[1], status: 'removed' }
		]);

		expect(groups[1].hasRemoval).toBe(false);
		expect(groups[1].rows[0].status).toBe('added');
	});

	it('leaves dates without target rows out of the diff', () => {
		const rows = [
			validRow({
				measuredAtLocal: '2026-09-01T07:00',
				systolic: '126',
				diastolic: '76',
				pulse: '70'
			})
		];
		// 対象範囲 (9/1〜9/5) の中だが、対象行が1件も無い日。置き換えないので確認画面に出さない。
		const existing = [
			{
				measuredAtLocal: '2026-09-03T07:20',
				systolic: 130,
				diastolic: 82,
				pulse: 66,
				memo: ''
			},
			{
				measuredAtLocal: '2026-09-05T07:00',
				systolic: 129,
				diastolic: 79,
				pulse: 67,
				memo: ''
			}
		];

		const groups = buildImportDiff(rows, existing);
		expect(groups.map((g) => g.date)).toEqual(['2026-09-01']);
		expect(groups[0].hasRemoval).toBe(false);
		expect(groups[0].rows.map((row) => row.status)).toEqual(['added']);
	});

	it('excludes rowError rows from the CSV side of the diff', () => {
		const rows = [validRow({ rowError: 'x' })];
		const groups = buildImportDiff(rows, []);
		expect(groups).toEqual([]);
	});

	it('lists a removed row before an added row at the same time', () => {
		const rows = [validRow({ measuredAtLocal: '2026-09-01T07:10', systolic: '124', memo: '新' })];
		const existing = [
			{
				measuredAtLocal: '2026-09-01T07:10',
				systolic: 123,
				diastolic: 80,
				pulse: undefined,
				memo: '元'
			},
			{
				measuredAtLocal: '2026-09-01T07:00',
				systolic: 120,
				diastolic: 80,
				pulse: undefined,
				memo: '朝'
			}
		];
		const [group] = buildImportDiff(rows, existing);
		expect(group.rows.map((row) => [row.measuredAtLocal.slice(11), row.status])).toEqual([
			['07:00', 'removed'],
			['07:10', 'removed'],
			['07:10', 'added']
		]);
	});

	it('matches duplicate content as a multiset (same-content rows pair off 1:1)', () => {
		const rows = [
			validRow({ measuredAtLocal: '2026-09-01T07:00', memo: 'dup' }),
			validRow({ measuredAtLocal: '2026-09-01T07:00', memo: 'dup' }),
			validRow({ measuredAtLocal: '2026-09-01T07:00', memo: 'dup' })
		];
		const existing = [
			{
				measuredAtLocal: '2026-09-01T07:00',
				systolic: 120,
				diastolic: 80,
				pulse: undefined,
				memo: 'dup'
			},
			{
				measuredAtLocal: '2026-09-01T07:00',
				systolic: 120,
				diastolic: 80,
				pulse: undefined,
				memo: 'dup'
			}
		];

		const groups = buildImportDiff(rows, existing);
		const statuses = groups[0].rows.map((r) => r.status).sort();
		// 3件のCSV行のうち2件は既存2件と対応して unchanged、残り1件だけ added。
		expect(statuses).toEqual(['added', 'unchanged', 'unchanged']);
	});
});

describe('deletionWarning', () => {
	function group(date: string, statuses: ImportDiffStatus[]): ImportDiffGroup {
		return {
			date,
			hasRemoval: statuses.includes('removed'),
			rows: statuses.map((status) => ({
				measuredAtLocal: `${date}T07:00`,
				systolic: 120,
				diastolic: 80,
				pulse: undefined,
				memo: '',
				status
			}))
		};
	}

	it('returns undefined when no record is removed', () => {
		expect(deletionWarning([group('2026-09-01', ['added', 'unchanged'])])).toBeUndefined();
	});

	it('warns without naming dates when any record is removed', () => {
		const groups = [group('2026-09-01', ['added']), group('2026-09-03', ['removed', 'added'])];
		expect(deletionWarning(groups)).toBe(m.import_preview_deletion_warning());
	});
});

describe('fetchExistingEntries', () => {
	beforeEach(() => {
		getMock.mockReset();
	});

	it('maps RecordResponse rows to diff entries', async () => {
		getMock.mockResolvedValueOnce({
			data: [
				{
					id: 1,
					version: 3,
					localMeasuredAt: '2026-09-01T09:00',
					systolic: 126,
					diastolic: 76,
					pulse: 70,
					memo: 'x'
				}
			],
			error: undefined,
			response: { ok: true }
		});

		const result = await fetchExistingEntries({ from: '2026-09-01', to: '2026-09-07' });
		if (!result.ok) throw new Error('expected success');
		expect(result.entries).toHaveLength(1);
		expect(result.entries[0].systolic).toBe(126);
		expect(result.entries[0].measuredAtLocal).toBe('2026-09-01T09:00');
		expect(result.entries[0]).toMatchObject({ id: 1, version: 3 });
		expect(getMock).toHaveBeenCalledWith('/api/v1/records', {
			params: { query: { from: '2026-09-01', to: '2026-09-07' } }
		});
	});

	it('returns a display message on failure', async () => {
		getMock.mockResolvedValueOnce({
			data: undefined,
			error: { error: { code: 'internal_error', message: 'x' } },
			response: { ok: false }
		});

		const result = await fetchExistingEntries({ from: '2026-09-01', to: '2026-09-07' });
		expect(result).toEqual({
			ok: false,
			message: m.error_internal_error(),
			code: 'internal_error'
		});
	});
});

describe('importRows', () => {
	beforeEach(() => {
		postMock.mockReset();
	});

	it('sends one request covering the full min/max date range of included rows, excluding error rows', async () => {
		postMock.mockResolvedValueOnce({
			data: { deleted: 2, created: [{}, {}] },
			error: undefined,
			response: { ok: true }
		});

		const included1 = validRow({
			measuredAtLocal: '2026-09-01T07:00',
			systolic: '126',
			diastolic: '76',
			pulse: '70',
			memo: 'x'
		});
		const included2 = validRow({ measuredAtLocal: '2026-09-05T21:00' });
		const rows = [
			included1,
			included2,
			validRow({ measuredAtLocal: '2026-09-10T07:00', rowError: 'x' })
		];
		const outcome = await importRows(rows, []);

		expect(outcome).toEqual({ ok: true, deleted: 2, created: 2 });
		expect(postMock).toHaveBeenCalledTimes(1);
		const body = postMock.mock.calls[0][1].body;
		// 対象範囲は対象行 (rowError 無し) の最小〜最大日付のみ。エラー行の日付 (9/10) は
		// 範囲にも送信内容にも影響しない。
		expect(body.from).toBe('2026-09-01');
		expect(body.to).toBe('2026-09-05');
		expect(body.records).toEqual([rowToRequest(included1), rowToRequest(included2)]);
	});

	it('reports a generic failure message without partial state (the request is atomic)', async () => {
		postMock.mockResolvedValueOnce({
			data: undefined,
			error: {
				error: { code: 'batch_validation_error', message: 'x', rows: [{ index: 0, message: 'x' }] }
			},
			response: { ok: false }
		});

		const outcome = await importRows([validRow()], []);
		expect(outcome).toEqual({
			ok: false,
			message: m.error_batch_validation_error(),
			code: 'batch_validation_error'
		});
	});

	it('sends the shown versions of existing records only for the days being replaced', async () => {
		postMock.mockResolvedValueOnce({
			data: { deleted: 1, created: [{}] },
			error: undefined,
			response: { ok: true }
		});
		const existing = (id: number, measuredAtLocal: string) => ({
			id,
			version: id * 10,
			measuredAtLocal,
			systolic: 120,
			diastolic: 80,
			pulse: undefined,
			memo: ''
		});

		await importRows(
			[
				validRow({ measuredAtLocal: '2026-09-01T07:00' }),
				validRow({ measuredAtLocal: '2026-09-03T07:00' })
			],
			[
				existing(1, '2026-09-01T08:00'),
				// 範囲の中でも、取り込む行が無い日は置き換えないので送らない。
				existing(2, '2026-09-02T08:00'),
				existing(3, '2026-09-03T21:00')
			]
		);

		expect(postMock.mock.calls[0][1].body.expected).toEqual([
			{ id: 1, version: 10 },
			{ id: 3, version: 30 }
		]);
	});

	it('throws if called with no included rows (callers must disable the button instead)', async () => {
		await expect(importRows([validRow({ rowError: 'x' })], [])).rejects.toThrow();
		expect(postMock).not.toHaveBeenCalled();
	});
});
