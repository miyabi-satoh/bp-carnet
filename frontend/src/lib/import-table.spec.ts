import { describe, expect, it } from 'vitest';
import { parseTableImport, tableMeasuredAt, tableSample, type TableColumns } from './import-table';
import { DEFAULT_PERIOD_THRESHOLDS } from '$lib/settings';

const TOKYO = 'Asia/Tokyo';

function columns(overrides: Partial<TableColumns> = {}): TableColumns {
	return {
		readable: true,
		delimiter: 'comma',
		headerLines: 1,
		dateOrder: 'ymd',
		utc: false,
		groups: [
			{ date: 0, time: null, systolic: 2, diastolic: 3, pulse: 4, memo: null, period: null }
		],
		...overrides
	};
}

describe('tableSample', () => {
	it('sends only the first lines, skipping blank lines and the BOM', () => {
		const lines = Array.from({ length: 40 }, (_, i) => `2026-02-${i + 1},120,80`);
		const sample = tableSample(`\uFEFFdate,sys,dia\r\n\r\n${lines.join('\n')}`);
		expect(sample.split('\n')).toHaveLength(12);
		expect(sample.startsWith('date,sys,dia\n2026-02-1,120,80')).toBe(true);
	});

	it('cuts long lines', () => {
		expect(tableSample(`a,${'x'.repeat(1000)}`)).toHaveLength(400);
	});
});

describe('tableMeasuredAt', () => {
	const read = (
		date: string,
		time?: string,
		order: TableColumns['dateOrder'] = 'ymd',
		utc = false
	) => tableMeasuredAt(date, time, order, utc, TOKYO);

	it.each([
		['2026-02-05 7:08', '2026-02-05', '07:08'],
		['2026/2/5 07:08:59', '2026-02-05', '07:08'],
		['2026年2月5日 7時8分', '2026-02-05', '07:08'],
		['2026/02/05(木) 19:30', '2026-02-05', '19:30'],
		['２０２６／０２／０５　７：０８', '2026-02-05', '07:08'],
		['2026-02-05T07:08', '2026-02-05', '07:08'],
		['20260205', '2026-02-05', ''],
		['Sep 28 2026', '2026-09-28', ''],
		['September 28, 2026, 7:45 PM', '2026-09-28', '19:45'],
		['28 Sep 2026 12:05 AM', '2026-09-28', '00:05'],
		['2026-02-05 午後7:08', '2026-02-05', '19:08']
	])('reads %s', (cell, measuredOnDate, time) => {
		expect(read(cell)).toEqual({ measuredOnDate, time });
	});

	it('reads the time from a separate column', () => {
		expect(read('Sep 28 2026', '07:45')).toEqual({ measuredOnDate: '2026-09-28', time: '07:45' });
	});

	it('uses the given order when the year comes last', () => {
		expect(read('02/05/2026 7:08', undefined, 'mdy')?.measuredOnDate).toBe('2026-02-05');
		expect(read('02/05/2026 7:08', undefined, 'dmy')?.measuredOnDate).toBe('2026-05-02');
	});

	it('follows the values when a number cannot be a month', () => {
		expect(read('25/12/2026', undefined, 'mdy')?.measuredOnDate).toBe('2026-12-25');
		expect(read('12/25/2026', undefined, 'dmy')?.measuredOnDate).toBe('2026-12-25');
	});

	it('converts UTC and offset times to the user time zone', () => {
		expect(read('2026-02-05 22:30:15.000', undefined, 'ymd', true)).toEqual({
			measuredOnDate: '2026-02-06',
			time: '07:30'
		});
		expect(read('2026-02-05 07:08:33 +0100')).toEqual({
			measuredOnDate: '2026-02-05',
			time: '15:08'
		});
		expect(read('2026-02-05T07:08:00Z')).toEqual({ measuredOnDate: '2026-02-05', time: '16:08' });
	});

	it.each([
		'',
		'yesterday',
		'2026-02-30',
		'2026-02-05 25:00',
		'2026-02-05 13:00 PM',
		'Foo 28 2026'
	])('rejects %s', (cell) => {
		expect(read(cell)).toBeNull();
	});
});

describe('parseTableImport', () => {
	it('reads an OMRON connect export', () => {
		const text =
			'測定日,タイムゾーン,最高血圧（mmHg）,最低血圧（mmHg）,脈拍（bpm）,機種\n' +
			'2026-02-05 7:08,Asia/Tokyo,140,106,89,HEM-7346T-AJC3\n' +
			'2026-02-05 21:40,Asia/Tokyo,132,98,,HEM-7346T-AJC3\n';
		const rows = parseTableImport(text, columns(), TOKYO, DEFAULT_PERIOD_THRESHOLDS);
		expect(
			rows.map((row) => [row.measuredOnDate, row.time, row.systolic, row.diastolic, row.pulse])
		).toEqual([
			['2026-02-05', '07:08', '140', '106', '89'],
			['2026-02-05', '21:40', '132', '98', '']
		]);
		expect(rows.every((row) => row.keep && !row.rowError)).toBe(true);
	});

	it('splits a row with morning and evening side by side, skipping empty measurements', () => {
		const text =
			'日付;朝 最高;朝 最低;朝 脈拍;夜 最高;夜 最低;夜 脈拍;メモ\n' +
			'2026/2/5;128;84;62;135;88;70;頭痛\n' +
			'2026/2/6;126;82;60;;;;\n' +
			'2026/2/7;;;;131;85;66;飲酒\n';
		const rows = parseTableImport(
			text,
			columns({
				delimiter: 'semicolon',
				groups: [
					{ date: 0, time: null, systolic: 1, diastolic: 2, pulse: 3, memo: 7, period: 'morning' },
					{
						date: 0,
						time: null,
						systolic: 4,
						diastolic: 5,
						pulse: 6,
						memo: 7,
						period: 'evening'
					}
				]
			}),
			TOKYO,
			DEFAULT_PERIOD_THRESHOLDS
		);
		expect(rows.map((row) => [row.measuredOnDate, row.periodHint, row.systolic, row.memo])).toEqual(
			[
				['2026-02-05', 'morning', '128', '頭痛'],
				['2026-02-05', 'evening', '135', ''],
				['2026-02-06', 'morning', '126', ''],
				// メモの列は1日に1つ。その行の最初の記録にだけ付く。
				['2026-02-07', 'evening', '131', '飲酒']
			]
		);
		// 時刻の無い行には、見出しの朝・夜の時間帯から仮の時刻を振る。
		expect(rows.every((row) => row.timeIsPlaceholder && row.time !== '')).toBe(true);
		expect(rows[0].time < '12:00' && rows[1].time > '12:00').toBe(true);
	});

	it('skips the lines before the data and strips units', () => {
		const text =
			'com.example.blood_pressure,1,2\n' +
			'start_time\tsystolic\tdiastolic\tpulse\n' +
			'2026-02-05 22:30:00.000\t120.0\t80 mmHg\t61 bpm\n';
		const rows = parseTableImport(
			text,
			columns({
				delimiter: 'tab',
				headerLines: 2,
				utc: true,
				groups: [
					{ date: 0, time: null, systolic: 1, diastolic: 2, pulse: 3, memo: null, period: null }
				]
			}),
			TOKYO,
			DEFAULT_PERIOD_THRESHOLDS
		);
		expect(rows).toHaveLength(1);
		expect(rows[0]).toMatchObject({
			measuredOnDate: '2026-02-06',
			time: '07:30',
			systolic: '120',
			diastolic: '80',
			pulse: '61',
			keep: true
		});
	});

	it('reads a pulse written as 0 or - as not measured', () => {
		const rows = parseTableImport(
			'date,x,sys,dia,pulse\n2026-02-05 7:08,,120,80,0\n2026-02-05 21:00,,122,81,-\n',
			columns(),
			TOKYO,
			DEFAULT_PERIOD_THRESHOLDS
		);
		expect(rows.map((row) => [row.pulse, row.keep])).toEqual([
			['', true],
			['', true]
		]);
	});

	it('keeps rows it cannot read as errors to fix', () => {
		const rows = parseTableImport(
			'date,x,sys,dia,pulse\nsomeday,,120,80,60\n2026-02-05 7:08,,120.5,80,60\n',
			columns(),
			TOKYO,
			DEFAULT_PERIOD_THRESHOLDS
		);
		expect(rows.map((row) => [row.measuredOnDate, row.systolic, row.keep])).toEqual([
			['', '120', false],
			['2026-02-05', '120.5', false]
		]);
		expect(rows.every((row) => row.rowError !== undefined)).toBe(true);
	});
});
