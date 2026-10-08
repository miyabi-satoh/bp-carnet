import { afterEach, describe, expect, it, vi } from 'vitest';
import {
	DAY_MINUTES_MAX,
	detectPeriodMode,
	formatDateOnly,
	formatDateRangeLabel,
	formatImportRowDateTime,
	formatFutureDateLabel,
	formatLocalDateTime,
	formatLocalDateTimeWithWeekday,
	formatOpenPeriodLabel,
	formatRecordCardDateTime,
	currentYearIn,
	localToday,
	isValidDateTime,
	parseDateParam,
	periodCovering,
	periodToShow,
	periodFor,
	splitPeriodLabel,
	minutesToClockTime,
	minutesToTimeText,
	toLocalDateTime,
	withHour,
	withMinute
} from '$lib/period';

describe('minutesToTimeText', () => {
	it('分を H:MM 表記にする', () => {
		expect(minutesToTimeText(0)).toBe('0:00');
		expect(minutesToTimeText(240)).toBe('4:00');
		expect(minutesToTimeText(545)).toBe('9:05');
	});

	// `<input type="time">` では表せない終了時刻。既定の夜の終了がこれにあたる。
	it('24:00 を表せる', () => {
		expect(minutesToTimeText(DAY_MINUTES_MAX)).toBe('24:00');
	});
});

describe('minutesToClockTime', () => {
	it('分を時もゼロ埋めした HH:MM 表記にする', () => {
		expect(minutesToClockTime(0)).toBe('00:00');
		expect(minutesToClockTime(420)).toBe('07:00');
		expect(minutesToClockTime(1439)).toBe('23:59');
	});
});

describe('withHour', () => {
	it('分を保ったまま時を変える', () => {
		expect(withHour(240, 9)).toBe(540);
		expect(withHour(270, 9)).toBe(570);
	});

	it('刻みに乗っていない分は切り捨てて刻みに乗せる', () => {
		expect(withHour(245, 9)).toBe(540);
		expect(withHour(285, 9)).toBe(570);
	});

	it('24 時を選ぶと 24:00 にする', () => {
		expect(withHour(270, 24)).toBe(DAY_MINUTES_MAX);
	});

	it('24:00 から時を変えると、その時の 0 分にする', () => {
		expect(withHour(DAY_MINUTES_MAX, 18)).toBe(1080);
	});
});

describe('withMinute', () => {
	it('時を保ったまま分を変える', () => {
		expect(withMinute(240, 30)).toBe(270);
		expect(withMinute(270, 0)).toBe(240);
	});

	it('24 時台は 24:00 を超えない', () => {
		expect(withMinute(DAY_MINUTES_MAX, 30)).toBe(DAY_MINUTES_MAX);
	});
});

describe('detectPeriodMode', () => {
	it('日曜始まりの1週なら週モードにし、基準日を初日にする', () => {
		const detected = detectPeriodMode('2026-09-06', '2026-09-12', new Date(2026, 9, 1));
		expect(detected?.mode).toBe('week');
		expect(detected && formatDateOnly(detected.anchor)).toBe('2026-09-06');
	});

	// 2026-02-01 は日曜なので、週 (2/1〜2/7) ではなく月として判定されることも確かめる。
	it('1か月ちょうどなら月モードにする', () => {
		expect(detectPeriodMode('2026-02-01', '2026-02-28', new Date(2026, 9, 1))?.mode).toBe('month');
	});

	it('週・月の境界と一致しなければ null を返す', () => {
		const today = new Date(2026, 9, 1);
		expect(detectPeriodMode('2026-09-07', '2026-09-13', today)).toBeNull();
		expect(detectPeriodMode('2026-08-01', '2026-08-30', today)).toBeNull();
		expect(detectPeriodMode('2026-09-06', '', today)).toBeNull();
		expect(detectPeriodMode('', '', today)).toBeNull();
	});

	it('今日を含む期間なら基準日を今日にする', () => {
		const detected = detectPeriodMode('2026-08-30', '2026-09-05', new Date(2026, 8, 3));
		expect(detected?.mode).toBe('week');
		expect(detected && formatDateOnly(detected.anchor)).toBe('2026-09-03');
	});
});

describe('periodCovering', () => {
	const today = new Date(2026, 8, 25);

	it('1週に収まれば、その週', () => {
		expect(periodCovering('2026-08-05', '2026-08-05', today)).toEqual({
			from: '2026-08-02',
			to: '2026-08-08'
		});
	});

	it('週をまたいでも1か月に収まれば、その月', () => {
		expect(periodCovering('2026-08-05', '2026-08-20', today)).toEqual({
			from: '2026-08-01',
			to: '2026-08-31'
		});
	});

	it('月をまたげば、範囲そのもの', () => {
		expect(periodCovering('2026-07-30', '2026-08-02', today)).toEqual({
			from: '2026-07-30',
			to: '2026-08-02'
		});
	});
});

describe('periodToShow', () => {
	const today = new Date(2026, 8, 25);

	it('期間の中なら null', () => {
		expect(periodToShow('2026-09-20', '2026-09-20', '2026-09-26', today)).toBeNull();
		expect(periodToShow('2026-09-26', '2026-09-20', '2026-09-26', today)).toBeNull();
	});

	it('週の表示中は、その日を含む週', () => {
		expect(periodToShow('2026-08-05', '2026-09-20', '2026-09-26', today)).toEqual({
			from: '2026-08-02',
			to: '2026-08-08'
		});
	});

	it('月の表示中は、その日を含む月', () => {
		expect(periodToShow('2026-07-15', '2026-09-01', '2026-09-30', today)).toEqual({
			from: '2026-07-01',
			to: '2026-07-31'
		});
	});

	it('任意期間の表示中は、その日を含む週', () => {
		expect(periodToShow('2026-09-25', '2026-09-01', '2026-09-10', today)).toEqual({
			from: '2026-09-20',
			to: '2026-09-26'
		});
	});

	it('片側が指定なしなら、そちらは限りなし', () => {
		expect(periodToShow('2020-01-01', '', '2026-09-26', today)).toBeNull();
		expect(periodToShow('2030-01-01', '2026-09-01', '', today)).toBeNull();
	});
});

describe('formatDateRangeLabel', () => {
	it('同じ年なら終わりの年を省く', () => {
		expect(formatDateRangeLabel(new Date(2026, 8, 6), new Date(2026, 8, 12))).toBe(
			'2026年9月6日 〜 9月12日'
		);
	});

	it('年をまたぐときは終わりにも年を付ける', () => {
		expect(formatDateRangeLabel(new Date(2026, 11, 27), new Date(2027, 0, 2))).toBe(
			'2026年12月27日 〜 2027年1月2日'
		);
	});
});

describe('splitPeriodLabel', () => {
	it('両端のある範囲は「〜」の後ろで分け、「〜」と空白は前の日付に付ける', () => {
		expect(splitPeriodLabel('2026年10月4日 〜 10月10日')).toEqual([
			'2026年10月4日 〜 ',
			'10月10日'
		]);
	});

	it('片側だけの範囲や月は分けない', () => {
		for (const label of ['2026年9月1日 〜', '〜 2026年9月7日', '2026年10月', '全期間']) {
			expect(splitPeriodLabel(label)).toEqual([label]);
		}
	});
});

describe('formatFutureDateLabel', () => {
	it('今年のうちなら年を省く', () => {
		expect(
			formatFutureDateLabel(
				new Date('2026-10-12T00:00+09:00'),
				'Asia/Tokyo',
				new Date('2026-09-12T00:00+09:00')
			)
		).toBe('10月12日');
	});

	it('年をまたぐときは年を付ける', () => {
		expect(
			formatFutureDateLabel(
				new Date('2027-01-19T00:00+09:00'),
				'Asia/Tokyo',
				new Date('2026-12-20T00:00+09:00')
			)
		).toBe('2027年1月19日');
	});

	it('日付も年の比較も、端末ではなく指定したタイムゾーンで行う', () => {
		// UTC ではまだ 2026年12月31日だが、日本時間では 2027年1月1日。
		expect(
			formatFutureDateLabel(
				new Date('2026-12-31T20:00Z'),
				'Asia/Tokyo',
				new Date('2026-12-20T00:00+09:00')
			)
		).toBe('2027年1月1日');
	});
});

describe('periodFor のラベル', () => {
	it('週は年月日の範囲、月は年月', () => {
		expect(periodFor('week', new Date(2026, 8, 9), new Date(2026, 8, 9)).label).toBe(
			'2026年9月6日 〜 9月12日'
		);
		expect(periodFor('month', new Date(2026, 7, 15), new Date(2026, 8, 9)).label).toBe('2026年8月');
	});
});

describe('formatLocalDateTimeWithWeekday', () => {
	it('曜日を付け、月日と時をゼロ埋めする', () => {
		expect(formatLocalDateTimeWithWeekday('2026-08-07T07:15')).toEqual({
			date: '2026/08/07(金)',
			time: '07:15'
		});
	});
});

describe('formatImportRowDateTime', () => {
	it('今年でない行にだけ年を付ける', () => {
		expect(formatImportRowDateTime('2024-12-31', '21:00', 'evening', false, '2026')).toBe(
			'2024年12月31日(火) 夜 21:00'
		);
		expect(formatImportRowDateTime('2026-09-07', '07:15', 'morning', false, '2026')).toBe(
			'9月7日(月) 朝 7:15'
		);
	});

	it('記録カードと同じ形で、曜日と朝/夜を付ける', () => {
		expect(formatImportRowDateTime('2026-09-07', '07:15', 'morning', false)).toBe(
			'9月7日(月) 朝 7:15'
		);
		expect(formatImportRowDateTime('2026-09-07', '12:30', undefined, false)).toBe(
			'9月7日(月) 12:30'
		);
	});

	// 読み取れなかった時刻に仮の値を入れているので、読み取れたように見せない (docs/import-export.md)。
	it('時刻が仮の値の行は、時刻を出さず朝/夜だけにする', () => {
		expect(formatImportRowDateTime('2026-09-07', '07:00', 'morning', true)).toBe('9月7日(月) 朝');
		expect(formatImportRowDateTime('2026-09-07', '12:30', undefined, true)).toBe('9月7日(月)');
	});

	// 読み取り・CSV から暦として読めない値が届く。その行こそ直してほしいので、一覧に出せなく
	// なるのではなく、入っている文字をそのまま見せる。
	it('暦として読めない日時は、入っている文字をそのまま返す', () => {
		expect(formatImportRowDateTime('', '', undefined, false)).toBe('');
		expect(formatImportRowDateTime('2026-09-07', '', undefined, false)).toBe('2026-09-07');
		expect(formatImportRowDateTime('2026-09-07', '7:30', undefined, false)).toBe('2026-09-07 7:30');
		expect(formatImportRowDateTime('2026/09/07', '07:30', undefined, false)).toBe(
			'2026/09/07 07:30'
		);
		expect(formatImportRowDateTime('2026-13-01', '07:30', undefined, false)).toBe(
			'2026-13-01 07:30'
		);
		expect(formatImportRowDateTime('2026-09-07', '24:00', undefined, false)).toBe(
			'2026-09-07 24:00'
		);
		expect(formatImportRowDateTime('2026-09-07', '07:30x', undefined, false)).toBe(
			'2026-09-07 07:30x'
		);
	});
});

describe('formatRecordCardDateTime', () => {
	const record = (localMeasuredAt: string, dayPeriod: 'morning' | 'evening' | null) => ({
		localMeasuredAt,
		dayPeriod
	});

	// 任意期間・全期間で年をまたぐと、年の無い日付はどの年か分からない。
	it('今年でない記録にだけ年を付ける', () => {
		expect(formatRecordCardDateTime(record('2026-09-07T07:15', 'morning'), '2026')).toBe(
			'9月7日(月) 朝 7:15'
		);
		expect(formatRecordCardDateTime(record('2025-01-05T07:15', 'morning'), '2026')).toBe(
			'2025年1月5日(日) 朝 7:15'
		);
	});

	it('曜日と朝/夜を付け、時はゼロ埋めしない', () => {
		expect(formatRecordCardDateTime(record('2026-09-07T07:15', 'morning'))).toBe(
			'9月7日(月) 朝 7:15'
		);
		expect(formatRecordCardDateTime(record('2026-09-06T21:05', 'evening'))).toBe(
			'9月6日(日) 夜 21:05'
		);
	});

	it('朝/夜のどちらにも入らない時刻は区分を付けない', () => {
		expect(formatRecordCardDateTime(record('2026-09-07T12:30', null))).toBe('9月7日(月) 12:30');
	});

	// `Date.UTC` は 0〜99 年を 1900 年代として扱う (0001-01-01 は月曜、1901-01-01 は火曜)。
	it('0〜99 年でも曜日がずれない', () => {
		expect(formatRecordCardDateTime(record('0001-01-01T07:00', 'morning'))).toBe(
			'1月1日(月) 朝 7:00'
		);
	});

	describe('端末のタイムゾーンが違うとき', () => {
		afterEach(() => {
			vi.unstubAllEnvs();
		});

		// 2026-03-08 02:30 は New York では夏時間の切り替えで存在しない。端末のタイムゾーンで
		// `Date` にすると 3:30 にずれる。
		it('端末側の夏時間の切り替えで時刻がずれない', () => {
			vi.stubEnv('TZ', 'America/New_York');
			expect(formatRecordCardDateTime(record('2026-03-08T02:30', null))).toBe('3月8日(日) 2:30');
		});
	});
});

describe('isValidDateTime', () => {
	it('実在する日時は true', () => {
		expect(isValidDateTime(2026, 9, 13)).toBe(true);
		expect(isValidDateTime(2026, 9, 13, 23, 59)).toBe(true);
		expect(isValidDateTime(2026, 12, 31)).toBe(true);
	});

	it('`new Date(year, ...)` が 1900 年代に寄せてしまう 0〜99 年は false', () => {
		expect(isValidDateTime(26, 1, 1)).toBe(false);
		expect(isValidDateTime(99, 12, 31)).toBe(false);
		expect(isValidDateTime(100, 1, 1)).toBe(true);
		expect(isValidDateTime(9999, 12, 31)).toBe(true);
		expect(isValidDateTime(10000, 1, 1)).toBe(false);
	});

	it('月・日が値域外なら false', () => {
		expect(isValidDateTime(2026, 0, 1)).toBe(false);
		expect(isValidDateTime(2026, 13, 1)).toBe(false);
		expect(isValidDateTime(2026, 9, 0)).toBe(false);
		expect(isValidDateTime(2026, 9, 31)).toBe(false);
		expect(isValidDateTime(2026, 2, 29)).toBe(false);
	});

	it('うるう年の 2月29日は 400年ルールまで含めて判定する', () => {
		expect(isValidDateTime(2024, 2, 29)).toBe(true);
		expect(isValidDateTime(2000, 2, 29)).toBe(true);
		expect(isValidDateTime(1900, 2, 29)).toBe(false);
	});

	it('時分が値域外なら false (省略時は 0:00 として扱う)', () => {
		expect(isValidDateTime(2026, 9, 13, 24, 0)).toBe(false);
		expect(isValidDateTime(2026, 9, 13, 0, 60)).toBe(false);
		expect(isValidDateTime(2026, 9, 13, -1, 0)).toBe(false);
		expect(isValidDateTime(2026, 9, 13, 0, -1)).toBe(false);
		expect(isValidDateTime(2026, 9, 13, 0, 0)).toBe(true);
	});
});

describe('parseDateParam', () => {
	it('YYYY-MM-DD の実在する日付はそのまま返す', () => {
		expect(parseDateParam('2026-08-01')).toBe('2026-08-01');
	});

	it('書式が違う・実在しない日付・値なしは空文字にする', () => {
		expect(parseDateParam('abc')).toBe('');
		expect(parseDateParam('0026-01-01')).toBe('');
		expect(parseDateParam('2026-8-1')).toBe('');
		expect(parseDateParam('2026-02-31')).toBe('');
		expect(parseDateParam('2026-13-01')).toBe('');
		expect(parseDateParam('')).toBe('');
		expect(parseDateParam(null)).toBe('');
	});
});

describe('formatOpenPeriodLabel', () => {
	it('両方空なら全期間、片側だけなら 〜 を付ける', () => {
		expect(formatOpenPeriodLabel('', '')).toBe('全期間');
		expect(formatOpenPeriodLabel('2026-09-01', '')).toBe('2026年9月1日 〜');
		expect(formatOpenPeriodLabel('', '2026-09-07')).toBe('〜 2026年9月7日');
	});

	it('両端とも指定されていれば undefined (両端の書き方は呼び出し側で組む)', () => {
		expect(formatOpenPeriodLabel('2026-09-01', '2026-09-07')).toBeUndefined();
	});
});

describe('toLocalDateTime', () => {
	afterEach(() => {
		vi.unstubAllEnvs();
	});

	it('瞬間を指定したタイムゾーンの日時にする', () => {
		expect(toLocalDateTime(new Date('2026-09-06T22:15:00Z'), 'Asia/Tokyo')).toBe(
			'2026-09-07T07:15'
		);
		expect(toLocalDateTime(new Date('2026-07-01T11:30:00Z'), 'America/New_York')).toBe(
			'2026-07-01T07:30'
		);
	});

	it('0 時を 00 と書く', () => {
		expect(toLocalDateTime(new Date('2026-09-06T15:00:00Z'), 'Asia/Tokyo')).toBe(
			'2026-09-07T00:00'
		);
	});

	it('端末のタイムゾーンに左右されない', () => {
		vi.stubEnv('TZ', 'America/New_York');
		expect(toLocalDateTime(new Date('2026-09-06T22:15:00Z'), 'Asia/Tokyo')).toBe(
			'2026-09-07T07:15'
		);
	});
});

describe('localToday', () => {
	it('指定したタイムゾーンでの日付にする', () => {
		const now = new Date('2026-09-06T15:30:00Z');
		expect(formatDateOnly(localToday('Asia/Tokyo', now))).toBe('2026-09-07');
		expect(formatDateOnly(localToday('America/New_York', now))).toBe('2026-09-06');
	});
});

describe('formatLocalDateTime', () => {
	it('年月日と時刻を表示する', () => {
		expect(formatLocalDateTime('2026-09-07T07:15')).toBe('2026/09/07 7:15');
	});
});

describe('currentYearIn', () => {
	it('タイムゾーンでの今年を返す', () => {
		// UTC では 2025年の大晦日の 20:00 だが、東京では 2026年の元日。
		const now = new Date('2025-12-31T20:00:00Z');
		expect(currentYearIn('Asia/Tokyo', now)).toBe('2026');
		expect(currentYearIn('UTC', now)).toBe('2025');
	});
});
