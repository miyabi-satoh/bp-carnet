import { describe, expect, it, vi } from 'vitest';
import * as m from '$lib/paraglide/messages.js';

vi.mock('$lib/api/client', () => ({ client: { GET: vi.fn(), POST: vi.fn() } }));
// 写真の縮小はブラウザの API に頼るので、読み取りの失敗の扱いだけを見るテストでは素通しにする。
vi.mock('$lib/ocr-image', () => ({ resizeImage: vi.fn(async (file: File) => file) }));
import { client } from '$lib/api/client';
import {
	carryOverMemos,
	confidenceLevel,
	extractFromPhoto,
	fetchOcrStatus,
	inferMeasuredOnYear,
	memoRowNote,
	memoRowStatus,
	memoRowsFromReadings,
	memoRowsYear,
	shiftMemoRowsYear,
	type MemoRow,
	type OcrReading
} from './ocr';
import type { ImportEntry } from '$lib/import';
import { DEFAULT_PERIOD_THRESHOLDS } from '$lib/settings';
import { respond } from '$lib/api/client.test-helpers';

describe('fetchOcrStatus', () => {
	it('成功したレスポンスをそのまま返す', async () => {
		vi.mocked(client.GET).mockReturnValue(
			respond(200, {
				enabled: true,
				quotas: [{ kind: 'free', purchasedAt: null, remainingPercent: 15 }],
				quotaLow: true,
				topupAvailable: true,
				consented: true
			})
		);
		expect(await fetchOcrStatus()).toEqual({
			enabled: true,
			quotas: [{ kind: 'free', purchasedAt: null, remainingPercent: 15 }],
			quotaLow: true,
			topupAvailable: true,
			consented: true
		});
	});

	it('無効なら、枠の残りと買い足しを出さない値にする', async () => {
		vi.mocked(client.GET).mockReturnValue(
			respond(200, {
				enabled: false,
				quotas: [{ kind: 'free', purchasedAt: null, remainingPercent: 15 }],
				quotaLow: true,
				topupAvailable: true,
				consented: true
			})
		);
		expect(await fetchOcrStatus()).toEqual({
			enabled: false,
			quotas: null,
			quotaLow: false,
			topupAvailable: false,
			consented: true
		});
	});

	it('失敗時は有効か・同意したかを「分からない」、買い足しを「出さない」にする (通信エラー等で誤って導線を出さない)', async () => {
		vi.mocked(client.GET).mockReturnValue(respond(500));
		expect(await fetchOcrStatus()).toEqual({
			enabled: null,
			quotas: null,
			quotaLow: false,
			topupAvailable: false,
			consented: null
		});
	});
});

describe('extractFromPhoto', () => {
	const photo = new File(['x'], 'photo.jpg', { type: 'image/jpeg' });
	const failWith = (status: number, error: object) =>
		vi.mocked(client.POST).mockReturnValue(respond(status, { error: { message: 'x', ...error } }));

	it('枠を使い切った・読み取りが無効の失敗は、選び直しても読み取れない (blocked)', async () => {
		for (const code of ['ocr_budget_exhausted', 'ocr_disabled']) {
			failWith(429, { code });
			expect(await extractFromPhoto(photo)).toMatchObject({ ok: false, code, blocked: true });
		}
	});

	it('読み取りの失敗は blocked にしない', async () => {
		failWith(502, { code: 'ocr_upstream_error' });
		expect(await extractFromPhoto(photo)).toMatchObject({ ok: false, blocked: false });
	});

	it('通信エラーは blocked にしない', async () => {
		vi.mocked(client.POST).mockRejectedValue(new TypeError('Failed to fetch'));
		expect(await extractFromPhoto(photo)).toEqual({
			ok: false,
			message: m.error_generic(),
			blocked: false
		});
	});
});

describe('confidenceLevel', () => {
	it('is high from 0.9', () => {
		expect(confidenceLevel(1)).toBe('high');
		expect(confidenceLevel(0.9)).toBe('high');
	});

	it('is medium from 0.75 (the low-confidence threshold) up to 0.9', () => {
		expect(confidenceLevel(0.89)).toBe('medium');
		expect(confidenceLevel(0.75)).toBe('medium');
	});

	it('is low below 0.75', () => {
		expect(confidenceLevel(0.74)).toBe('low');
		expect(confidenceLevel(0)).toBe('low');
	});
});

describe('inferMeasuredOnYear', () => {
	const now = new Date(2026, 8, 7); // 2026-09-07 (ローカル時刻)

	it('assumes the current year when the resulting date is not in the future', () => {
		expect(inferMeasuredOnYear('09-05', now)).toBe('2026-09-05');
	});

	it('treats today as not-yet-future (does not roll back)', () => {
		expect(inferMeasuredOnYear('09-07', now)).toBe('2026-09-07');
	});

	it('rolls back one year when the current-year date would be in the future', () => {
		// 12月分のメモを年明けに読み込んだケースを想定。
		expect(inferMeasuredOnYear('12-31', now)).toBe('2025-12-31');
	});

	it('今年・去年のどちらにも無い月日は、今日にせず空にする (行を「要確認」にするため)', () => {
		// "02-31" は Date コンストラクタに渡すと繰り上がって 3/2 や 3/3 になってしまうが、
		// そもそも実在しない日付なので OCR の誤読とみなす。
		// 2026年・2025年 (今年・去年) はどちらもうるう年ではないため "02-29" もここに含まれる。
		expect(inferMeasuredOnYear('02-31', now)).toBe('');
		expect(inferMeasuredOnYear('00-00', now)).toBe('');
		expect(inferMeasuredOnYear('13-01', now)).toBe('');
		expect(inferMeasuredOnYear('02-29', now)).toBe('');
	});

	it('resolves a leap day to last year when this year does not have one but last year does', () => {
		const nonLeapNow = new Date(2025, 8, 7); // 2025-09-07 (2025年はうるう年でない、2024年はうるう年)
		expect(inferMeasuredOnYear('02-29', nonLeapNow)).toBe('2024-02-29');
	});

	it('"MM-DD" でなければ、今日にせず空にする', () => {
		expect(inferMeasuredOnYear('', now)).toBe('');
		expect(inferMeasuredOnYear(null, now)).toBe('');
		expect(inferMeasuredOnYear(undefined, now)).toBe('');
	});
});

describe('手帳の年 (docs/ocr.md)', () => {
	const now = new Date(2026, 8, 7);
	const rowsFor = (measuredOn: (string | null)[]) =>
		memoRowsFromReadings(
			measuredOn.map((date) => reading({ measuredOn: date })),
			DEFAULT_PERIOD_THRESHOLDS,
			now
		);

	it('年の変わり目をまたぐページは、新しいほうの年を欄に出す', () => {
		expect(memoRowsYear(rowsFor(['12-30', '01-02']))).toBe(2026);
		expect(memoRowsYear(rowsFor(['09-01']))).toBe(2026);
	});

	it('日付の入った行が無ければ、年は無い', () => {
		expect(memoRowsYear(rowsFor([null]))).toBeNull();
	});

	it('選んだ年へ、すべての行を同じ年数だけずらす。日付の無い行はそのまま', () => {
		const rows = rowsFor(['12-30', '01-02', null]);
		shiftMemoRowsYear(rows, 2019);

		expect(rows.map((row) => row.measuredOnDate)).toEqual(['2018-12-30', '2019-01-02', '']);
		expect(memoRowsYear(rows)).toBe(2019);
	});

	it('本人がシートで入れた日付は動かさず、欄の年にも数えない', () => {
		const rows = rowsFor(['09-01', '09-02', null]);
		rows[1].measuredOnDate = '2030-01-01';
		rows[2].measuredOnDate = '2019-03-05';
		shiftMemoRowsYear(rows, 2019);

		expect(rows.map((row) => row.measuredOnDate)).toEqual([
			'2019-09-01',
			'2030-01-01',
			'2019-03-05'
		]);
		expect(memoRowsYear(rows)).toBe(2019);
	});

	it('日付も時刻も読めなかった行には、仮の時刻を振ったことにしない', () => {
		const [row] = memoRowsFromReadings(
			[reading({ measuredOn: null, time: null })],
			DEFAULT_PERIOD_THRESHOLDS,
			now
		);
		expect(row.time).toBe('');
		expect(row.timeIsPlaceholder).toBe(false);
	});

	it('ずらした先に無いうるう日は「要確認」にし、戻せば解ける', () => {
		const rows = memoRowsFromReadings(
			[reading({ measuredOn: '02-29' })],
			DEFAULT_PERIOD_THRESHOLDS,
			new Date(2025, 8, 7)
		);
		shiftMemoRowsYear(rows, 2023);
		expect(rows[0].measuredOnDate).toBe('2023-02-29');
		expect(memoRowStatus(rows[0])).toBe('error');

		shiftMemoRowsYear(rows, 2020);
		expect(rows[0].measuredOnDate).toBe('2020-02-29');
		expect(memoRowStatus(rows[0])).toBe('ok');
	});

	it('日付を読み取れなかった行は、今日にせず「要確認」にして外し、日付を入れるよう添える', () => {
		const [row] = rowsFor([null]);

		expect(row.measuredOnDate).toBe('');
		expect(memoRowStatus(row)).toBe('error');
		expect(row.keep).toBe(false);
		expect(memoRowNote(row)).toBe(m.import_rows_note_date_unread());
	});
});

function reading(overrides: Partial<OcrReading> = {}): OcrReading {
	return {
		measuredOn: '09-05',
		time: '07:30',
		systolic: 128,
		diastolic: 82,
		pulse: 70,
		confidence: 0.9,
		...overrides
	};
}

describe('memoRowStatus', () => {
	const row = (overrides: Partial<MemoRow>): MemoRow => ({
		keep: true,
		measuredOnDate: '2026-09-05',
		inferredDate: '2026-09-05',
		time: '07:30',
		systolic: '128',
		diastolic: '82',
		pulse: '',
		confidence: 0.9,
		note: '',
		memo: '',
		...overrides
	});

	it('CSV と同じ判定に、読み取りの自信の低さを足す', () => {
		expect(memoRowStatus(row({}))).toBe('ok');
		expect(memoRowStatus(row({ confidence: 0.5 }))).toBe('warn');
		// 仮の時刻を振っただけでは知らせない (docs/ocr.md)。
		expect(memoRowStatus(row({ timeIsPlaceholder: true }))).toBe('ok');
		// 直さないと取り込めない行が先。
		expect(memoRowStatus(row({ confidence: 0.5, rowError: 'x' }))).toBe('error');
	});
});

describe('memoRowsFromReadings', () => {
	const now = new Date(2026, 8, 7);

	it('carries over the reading values as editable row state', () => {
		const [row] = memoRowsFromReadings([reading()], DEFAULT_PERIOD_THRESHOLDS, now);
		expect(row).toMatchObject({
			measuredOnDate: '2026-09-05',
			time: '07:30',
			systolic: '128',
			diastolic: '82',
			pulse: '70',
			confidence: 0.9
		});
	});

	it('unchecks low-confidence rows by default (< 0.75)', () => {
		const [highConf, lowConf] = memoRowsFromReadings(
			[reading({ confidence: 0.8 }), reading({ confidence: 0.6 })],
			DEFAULT_PERIOD_THRESHOLDS,
			now
		);
		expect(highConf.keep).toBe(true);
		expect(lowConf.keep).toBe(false);
	});

	it('treats pulse 0 as not-entered, matching the single-shot dialog', () => {
		const [row] = memoRowsFromReadings([reading({ pulse: 0 })], DEFAULT_PERIOD_THRESHOLDS, now);
		expect(row.pulse).toBe('');
	});

	it('balances placeholder times between morning and evening per date when time is missing', () => {
		const rows = memoRowsFromReadings(
			[
				reading({ measuredOn: '09-05', time: '' }),
				reading({ measuredOn: '09-05', time: '' }),
				reading({ measuredOn: '09-06', time: '' })
			],
			DEFAULT_PERIOD_THRESHOLDS,
			now
		);
		// 日付ごとに件数を数え直すので、1行しかない 09-06 は朝になる。
		expect(rows.map((row) => row.time)).toEqual(['07:00', '21:00', '07:00']);
	});

	it('puts an untimed row in the period written beside it', () => {
		const rows = memoRowsFromReadings(
			[reading({ measuredOn: '09-05', time: '', period: 'evening' })],
			DEFAULT_PERIOD_THRESHOLDS,
			now
		);
		expect(rows.map((row) => [row.time, row.periodHint])).toEqual([['21:00', 'evening']]);
	});

	it('sets rowError for readings outside the valid range (CSVインポートと共通の値域チェック)', () => {
		const [row] = memoRowsFromReadings(
			[reading({ systolic: 300 })],
			DEFAULT_PERIOD_THRESHOLDS,
			now
		);
		expect(row.rowError).toBeTruthy();
	});

	it('leaves rowError unset for readings within the valid range', () => {
		const [row] = memoRowsFromReadings([reading()], DEFAULT_PERIOD_THRESHOLDS, now);
		expect(row.rowError).toBeUndefined();
	});

	it('takes the memo text written in the notebook, keeping Gemini notes separate', () => {
		const [withMemo, withoutMemo] = memoRowsFromReadings(
			[reading({ memo: '頭痛あり', note: '字がかすれている' }), reading()],
			DEFAULT_PERIOD_THRESHOLDS,
			now
		);
		expect(withMemo).toMatchObject({ memo: '頭痛あり', note: '字がかすれている' });
		expect(withoutMemo.memo).toBe('');
	});
});

describe('carryOverMemos', () => {
	function memoRow(overrides: Partial<MemoRow> = {}): MemoRow {
		return {
			keep: true,
			measuredOnDate: '2026-09-05',
			inferredDate: '2026-09-05',
			time: '07:00',
			systolic: '128',
			diastolic: '82',
			pulse: '',
			confidence: 0.9,
			note: '',
			memo: '',
			...overrides
		};
	}

	function entry(overrides: Partial<ImportEntry> = {}): ImportEntry {
		return {
			measuredAtLocal: '2026-09-05T07:00',
			systolic: 128,
			diastolic: 82,
			pulse: undefined,
			memo: '薬を飲んだ',
			...overrides
		};
	}

	it('carries the memo of a record in the same date, period and blood pressure', () => {
		const rows = [memoRow({ time: '07:30' })];
		carryOverMemos(
			rows,
			[entry({ measuredAtLocal: '2026-09-05T06:15' })],
			DEFAULT_PERIOD_THRESHOLDS
		);
		expect(rows[0].memo).toBe('薬を飲んだ');
	});

	it('prefers the memo written on screen over the text read from the notebook', () => {
		const rows = [memoRow({ memo: '頭痛' })];
		carryOverMemos(rows, [entry()], DEFAULT_PERIOD_THRESHOLDS);
		expect(rows[0].memo).toBe('薬を飲んだ');
	});

	it('does not carry across a different date, period or blood pressure', () => {
		const rows = [memoRow({ memo: '頭痛' })];
		carryOverMemos(
			rows,
			[
				entry({ measuredAtLocal: '2026-09-06T07:00' }),
				entry({ measuredAtLocal: '2026-09-05T21:00' }),
				entry({ systolic: 130 }),
				entry({ diastolic: 80 })
			],
			DEFAULT_PERIOD_THRESHOLDS
		);
		expect(rows[0].memo).toBe('頭痛');
	});

	it('ignores records without a memo, keeping the text read from the notebook', () => {
		const rows = [memoRow({ memo: '頭痛' })];
		carryOverMemos(rows, [entry({ memo: '' })], DEFAULT_PERIOD_THRESHOLDS);
		expect(rows[0].memo).toBe('頭痛');
	});

	it('gives each record to one row, matching the same time first', () => {
		const rows = [memoRow({ time: '07:00' }), memoRow({ time: '07:01' })];
		carryOverMemos(
			rows,
			[
				entry({ measuredAtLocal: '2026-09-05T07:01', memo: '2回目' }),
				entry({ measuredAtLocal: '2026-09-05T07:00', memo: '1回目' })
			],
			DEFAULT_PERIOD_THRESHOLDS
		);
		expect(rows.map((row) => row.memo)).toEqual(['1回目', '2回目']);
	});

	it('skips rows that canCarry rejects, leaving the record for the next matching row', () => {
		const rows = [memoRow({ memo: '入力中' }), memoRow({ time: '07:01' })];
		carryOverMemos(rows, [entry()], DEFAULT_PERIOD_THRESHOLDS, (row) => row.memo !== '入力中');
		expect(rows.map((row) => row.memo)).toEqual(['入力中', '薬を飲んだ']);
	});

	it('does not carry to a row that cannot be imported', () => {
		const rows = [
			memoRow({ systolic: '128.0' }),
			memoRow({ time: '07:01', rowError: '上の血圧は整数で入力してください' })
		];
		carryOverMemos(
			rows,
			[entry(), entry({ measuredAtLocal: '2026-09-05T07:01' })],
			DEFAULT_PERIOD_THRESHOLDS
		);
		expect(rows.map((row) => row.memo)).toEqual(['', '']);
	});

	it('does not carry to a row outside both periods', () => {
		const rows = [memoRow({ time: '12:00' })];
		carryOverMemos(
			rows,
			[entry({ measuredAtLocal: '2026-09-05T12:00' })],
			DEFAULT_PERIOD_THRESHOLDS
		);
		expect(rows[0].memo).toBe('');
	});
});
