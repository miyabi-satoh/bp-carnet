import type { components } from '$lib/api/schema';
import * as m from '$lib/paraglide/messages.js';
import { getLocale } from '$lib/paraglide/runtime.js';
import { inRange } from '$lib/utils';

export type DayPeriod = components['schemas']['DayPeriod'];
type RecordListItem = components['schemas']['RecordListItem'];

const pad = (n: number) => String(n).padStart(2, '0');

/** 瞬間 `date` を、`timeZone` での `YYYY-MM-DDTHH:MM` (API の `localMeasuredAt` の形) にする。 */
export function toLocalDateTime(date: Date, timeZone: string): string {
	const parts = new Intl.DateTimeFormat('en-US', {
		timeZone,
		year: 'numeric',
		month: 'numeric',
		day: 'numeric',
		hour: 'numeric',
		minute: 'numeric',
		// 既定の hour12: false は、エンジンによって 0 時を「24」と書くため h23 を明示する。
		hourCycle: 'h23'
	}).formatToParts(date);
	const value = (type: Intl.DateTimeFormatPartTypes) =>
		Number(parts.find((part) => part.type === type)?.value);
	const year = String(value('year')).padStart(4, '0');
	return `${year}-${pad(value('month'))}-${pad(value('day'))}T${pad(value('hour'))}:${pad(value('minute'))}`;
}

/** `timeZone` での今日 (その日の 0:00 の `Date`)。週/月の計算は `Date` の暦の値だけを使う。 */
export function localToday(timeZone: string, now = new Date()): Date {
	return parseLocalDate(toLocalDateTime(now, timeZone).slice(0, 10));
}

/** 日時を表示用文字列にする。グラフのツールチップに使う。 */
export function formatDateTime(date: Date): string {
	return date.toLocaleString(getLocale(), { dateStyle: 'medium', timeStyle: 'short' });
}

/** `YYYY-MM-DDTHH:MM` がちょうどその形 (時・分もゼロ埋め) で、暦として実在するか。 */
export function isLocalDateTime(local: string): boolean {
	const match = /^(\d{4})-(\d{2})-(\d{2})T(\d{2}):(\d{2})$/.exec(local);
	if (!match) return false;
	const [year, month, day, hour, minute] = match.slice(1).map(Number);
	return isValidDateTime(year, month, day, hour, minute);
}

/** `YYYY-MM-DDTHH:MM` (サーバーが返す `localMeasuredAt`) を暦の値に分ける。 */
function parseLocalDateTime(local: string) {
	const match = /^(\d{4})-(\d{2})-(\d{2})T(\d{2}):(\d{2})/.exec(local);
	if (!match) throw new Error(`unexpected local datetime: ${local}`);
	const [year, month, day, hour, minute] = match.slice(1).map(Number);
	return { year, month, day, hour, minute };
}

/** 暦の値をそのまま持つ UTC の `Date`。`timeZone: 'UTC'` で書式化して使う。端末のタイムゾーンで
 * `Date` にすると、端末側の夏時間の切り替えで存在しない時刻がずれるため。年は `Date.UTC` だと
 * 0〜99 が 1900 年代になるため `setUTCFullYear` で設定する。 */
function utcDateOf({
	year,
	month,
	day,
	hour = 0,
	minute = 0
}: {
	year: number;
	month: number;
	day: number;
	hour?: number;
	minute?: number;
}): Date {
	const date = new Date(0);
	date.setUTCFullYear(year, month - 1, day);
	date.setUTCHours(hour, minute);
	return date;
}

/** `localMeasuredAt` の表示 (例: `2026/09/07 7:15`)。削除の確認文や読み上げラベルに使う。 */
export function formatLocalDateTime(local: string): string {
	return utcDateOf(parseLocalDateTime(local)).toLocaleString(getLocale(), {
		timeZone: 'UTC',
		dateStyle: 'medium',
		timeStyle: 'short'
	});
}

/** グラフのX軸目盛り用の短い日付表示 (例: `9/7`)。 */
export function formatMonthDay(date: Date): string {
	return `${date.getMonth() + 1}/${date.getDate()}`;
}

/** 曜日付きの日付表示 (例: `2026年9月7日(月)`、`withYear` が false なら `9月7日(月)`)。
 * `timeZone` を省くと端末のタイムゾーンで表示する。 */
export function formatDateWithWeekday(date: Date, withYear = true, timeZone?: string): string {
	return date.toLocaleDateString(getLocale(), {
		timeZone,
		...(withYear && { year: 'numeric' }),
		month: 'long',
		day: 'numeric',
		weekday: 'short'
	});
}

const DAY_PERIOD_LABELS: Record<DayPeriod, () => string> = {
	morning: m.record_day_period_morning_label,
	evening: m.record_day_period_evening_label
};

/** `timeZone` での今年 (`YYYY`)。今年でない日付にだけ年を付けるときの比べる相手。 */
export function currentYearIn(timeZone: string, now = new Date()): string {
	return toLocalDateTime(now, timeZone).slice(0, 4);
}

/** 曜日付きの日付に朝/夜と時刻を添える (例: `9月7日(月) 朝 7:15`)。`dayPeriod` が無ければ区分を
 * 付けず、`withTime` が false なら時刻を付けない。`thisYear` (`YYYY`) と年が違えば年を付ける
 * (`2025年1月5日(日) 朝 7:15`。任意期間・全期間や昔の手帳の取り込みで、どの年か分からなくなるため)。 */
function formatDayPeriodDateTime(
	local: string,
	dayPeriod: DayPeriod | null | undefined,
	withTime: boolean,
	thisYear: string | undefined
): string {
	const { hour, minute, ...date } = parseLocalDateTime(local);
	const withYear = thisYear !== undefined && local.slice(0, 4) !== thisYear;
	return [
		formatDateWithWeekday(utcDateOf(date), withYear, 'UTC'),
		dayPeriod && DAY_PERIOD_LABELS[dayPeriod](),
		withTime && `${hour}:${pad(minute)}`
	]
		.filter(Boolean)
		.join(' ');
}

/** 記録カードの日時 (例: `9月7日(月) 朝 7:15`)。どちらの区分にも属さない時刻は区分を付けない。
 * `thisYear` と年が違えば年を付ける。 */
export function formatRecordCardDateTime(
	{ localMeasuredAt, dayPeriod }: Pick<RecordListItem, 'localMeasuredAt' | 'dayPeriod'>,
	thisYear?: string
): string {
	return formatDayPeriodDateTime(localMeasuredAt, dayPeriod, true, thisYear);
}

/** 取り込みの行の測定日時 (記録カードと同じ形)。`dayPeriod` が分かれば時刻の前に出す。
 * 時刻が仮の値の行は、時刻を出さず朝/夜だけにする (読み取れなかった時刻を、読み取れたように
 * 見せないため。docs/import-export.md)。 */
export function formatImportRowDateTime(
	measuredOnDate: string,
	time: string,
	dayPeriod: DayPeriod | undefined,
	timeIsPlaceholder: boolean,
	thisYear?: string
): string {
	const local = `${measuredOnDate}T${time}`;
	// 日時は読み取り・CSV から来るので、暦として読めない値も届く (空・`2026-13-01`・`7:30` など)。
	// その行こそ直してほしいので、一覧に出せないのではなく、入っている文字をそのまま見せる。
	if (!isLocalDateTime(local)) {
		return [measuredOnDate, time].filter((part) => part !== '').join(' ');
	}
	return formatDayPeriodDateTime(local, dayPeriod, !timeIsPlaceholder, thisYear);
}

/** 年月日の表示 (例: `2026年9月6日`、`withYear` が false なら `9月6日`)。期間のラベルに使う。
 * `timeZone` を省くと端末のタイムゾーンで表示する。 */
export function formatDateLabel(date: Date, withYear = true, timeZone?: string): string {
	return date.toLocaleDateString(
		getLocale(),
		withYear
			? { timeZone, year: 'numeric', month: 'long', day: 'numeric' }
			: { timeZone, month: 'long', day: 'numeric' }
	);
}

/** これから来る日付の `timeZone` でのラベル。年が今年と違うときだけ年を付ける (30日後が翌年になると、
 * 年が無い「1月19日」が10か月前の日付に見えてしまうため)。 */
export function formatFutureDateLabel(date: Date, timeZone: string, now = new Date()): string {
	return formatDateLabel(
		date,
		currentYearIn(timeZone, date) !== currentYearIn(timeZone, now),
		timeZone
	);
}

/** 期間の片側・両側が指定されていないときのラベル (「全期間」「2026年9月1日 〜」「〜 2026年9月7日」)。`from`/`to` は
 * YYYY-MM-DD で、空文字は指定なし。両端とも指定されていれば `undefined` を返す (両端のときの年の省き方は
 * 画面ごとに違うため、呼び出し側で組む)。 */
export function formatOpenPeriodLabel(from: string, to: string): string | undefined {
	if (!from && !to) return m.period_all_label();
	if (!to) return m.period_from_only_label({ from: formatDateLabel(parseLocalDate(from)) });
	if (!from) return m.period_to_only_label({ to: formatDateLabel(parseLocalDate(to)) });
	return undefined;
}

/** 文言を差し込み口で切り分けるための印。文言には現れない文字にする。 */
const MARK = '\u241f';

/** 期間のラベルを、行を分けてよい所で切り分ける。両端のある範囲は「〜」の後ろだけで分け、「〜」と後ろの空白は前の日付に付ける
 * (`2026年9月6日 〜 ` と `9月12日`。つなげると元のラベルになる)。それ以外は分けない。画面が狭いとき、日付の途中で折り返さないようにするため
 * (iOS の Safari は文節での折り返しの指定が効かない)。 */
export function splitPeriodLabel(label: string): string[] {
	const [, separator] = m.period_range_label({ from: MARK, to: MARK }).split(MARK);
	const at = label.indexOf(separator);
	if (at <= 0 || at + separator.length >= label.length) return [label];
	return [label.slice(0, at + separator.length), label.slice(at + separator.length)];
}

/** 期間のラベル (例: `2026年9月6日 〜 9月12日`)。終わりの年は、始まりと年が違うときだけ付ける
 * (スマホの画面幅で1行に収めるため)。 */
export function formatDateRangeLabel(from: Date, to: Date): string {
	return m.period_range_label({
		from: formatDateLabel(from),
		to: formatDateLabel(to, from.getFullYear() !== to.getFullYear())
	});
}

/** 曜日付きで、月日と時をゼロ埋めした `localMeasuredAt` の表示を、日付と時刻に分けて返す
 * (例: `2026/08/07(金)` と `07:15`)。印刷用レポートの表に使う。スマホの幅では2段に並べるため分ける。 */
export function formatLocalDateTimeWithWeekday(local: string): { date: string; time: string } {
	const { year, month, day, hour, minute } = parseLocalDateTime(local);
	const weekday = utcDateOf({ year, month, day }).toLocaleDateString(getLocale(), {
		timeZone: 'UTC',
		weekday: 'short'
	});
	return {
		date: `${year}/${pad(month)}/${pad(day)}(${weekday})`,
		time: `${pad(hour)}:${pad(minute)}`
	};
}

const DAYS_IN_MONTH = [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];

/** このアプリが日付として扱える年。下限が 100 なのは、`parseLocalDate` 等が使う
 * `new Date(year, ...)` が 0〜99 を 1900 年代として解釈するため。判定だけ通すと、
 * 表示や期間の計算が指定とは別の年になる。 */
const YEAR_RANGE = [100, 9999] as const;

/** `year`/`month`(1-12)/`day`/`hour`/`minute` が実在する日時を指しているか
 * (時分の省略は 0:00)。
 *
 * ADR: `Date` に通して値が変わらないかを見る方式は採らない。`Date` は月日・時分の
 * オーバーフローを繰り上げて正規化する (例: 2月31日 → 3月3日) ので判定自体はできるが、
 * 夏時間の切り替えが 0:00 に起こる地域では実在する日付でも生成結果がずれ、端末の
 * タイムゾーンで結果が変わる。暦の計算だけで判定する。 */
export function isValidDateTime(
	year: number,
	month: number,
	day: number,
	hour = 0,
	minute = 0
): boolean {
	if (!inRange(year, YEAR_RANGE) || !inRange(month, [1, 12])) return false;
	if (!inRange(hour, [0, 23]) || !inRange(minute, [0, 59])) return false;
	const leapYear = (year % 4 === 0 && year % 100 !== 0) || year % 400 === 0;
	const lastDay = month === 2 && leapYear ? 29 : DAYS_IN_MONTH[month - 1];
	return inRange(day, [1, lastDay]);
}

/** URLクエリの日付 (YYYY-MM-DD) を読み取る。書式が違う・実在しない日付なら空文字 (指定なし) を返す。
 * 手で書き換えたURL等で、見出しが「Invalid Date」になったり、取得する期間が別の日にずれたりするのを避けるため。 */
export function parseDateParam(value: string | null): string {
	const match = /^(\d{4})-(\d{2})-(\d{2})$/.exec(value ?? '');
	if (!match) return '';
	return isValidDateTime(Number(match[1]), Number(match[2]), Number(match[3])) ? match[0] : '';
}

/** `YYYY-MM-DD` をその日の 00:00 の `Date` にする。`new Date('2026-09-08')` は
 * UTC 解釈になりタイムゾーンによって前日にずれるため使わない。 */
export function parseLocalDate(dateStr: string): Date {
	const [year, month, day] = dateStr.split('-').map(Number);
	return new Date(year, month - 1, day);
}

/** 朝/夜の閾値として指定できる分数の上限 (24:00)。バックエンドの `DAY_MINUTES_RANGE` と揃える。 */
export const DAY_MINUTES_MAX = 1440;

/** 「0:00 からの経過分数」を `H:MM` 表記にする (例: 240 -> `4:00`、1440 -> `24:00`)。
 * デザイン上、設定画面の入力欄は時刻をこの表記で扱う。 */
export function minutesToTimeText(minutes: number): string {
	return `${Math.floor(minutes / 60)}:${pad(minutes % 60)}`;
}

/** 「0:00 からの経過分数」を、時もゼロ埋めした `HH:MM` 表記にする (例: 420 -> `07:00`)。
 * `<input type="time">` の値の形式。 */
export function minutesToClockTime(minutes: number): string {
	return `${pad(Math.floor(minutes / 60))}:${pad(minutes % 60)}`;
}

/** 時もゼロ埋めした `HH:MM` 表記を「0:00 からの経過分数」にする (`minutesToClockTime` の逆)。
 * 形式が違う・実在しない時刻 (`07:60`・`24:00` など) なら `undefined`。 */
export function parseClockTime(time: string): number | undefined {
	const match = /^(\d{2}):(\d{2})$/.exec(time);
	if (!match) return undefined;
	const [hour, minute] = [Number(match[1]), Number(match[2])];
	return inRange(hour, [0, 23]) && inRange(minute, [0, 59]) ? hour * 60 + minute : undefined;
}

/** 設定画面で朝/夜の時刻として選べる分の刻み。 */
export const TIME_PICKER_STEP_MIN = 30;

/** 「0:00 からの経過分数」の時だけを選び直す。分は保つが、刻みに乗っていなければ切り捨てて乗せる
 * (選択肢に無い分を保存しないため)。24 時台は 24:00 だけにする。 */
export function withHour(minutes: number, hour: number): number {
	const minute = Math.floor((minutes % 60) / TIME_PICKER_STEP_MIN) * TIME_PICKER_STEP_MIN;
	return Math.min(hour * 60 + minute, DAY_MINUTES_MAX);
}

/** 「0:00 からの経過分数」の分だけを選び直す (時はそのまま)。24 時台は 24:00 だけにする。 */
export function withMinute(minutes: number, minute: number): number {
	return Math.min(Math.floor(minutes / 60) * 60 + minute, DAY_MINUTES_MAX);
}

/** `Date` の暦の日付を `<input type="date">` 用文字列 (YYYY-MM-DD) にする。
 * `Date#toISOString()` は UTC の日付になるため使わない。 */
export function formatDateOnly(date: Date): string {
	return `${date.getFullYear()}-${pad(date.getMonth() + 1)}-${pad(date.getDate())}`;
}

/** 日曜始まりの週の開始日 (00:00) を返す。 */
function startOfWeek(date: Date): Date {
	return new Date(date.getFullYear(), date.getMonth(), date.getDate() - date.getDay());
}

/** 週/月ナビゲーションで表示する期間。 */
export type Period = {
	from: Date;
	to: Date;
	label: string;
	/** 今日を含む週/月と一致するかどうか。 */
	isCurrent: boolean;
};

/** `anchor` を含む週/月の期間 (開始日・終了日・表示ラベル・`today` を含むかどうか) を計算する。
 * `today` は個人設定のタイムゾーンでの今日 (`localToday`)。 */
export function periodFor(mode: 'week' | 'month', anchor: Date, today: Date): Period {
	if (mode === 'week') {
		const start = startOfWeek(anchor);
		const end = new Date(start.getFullYear(), start.getMonth(), start.getDate() + 6);
		return {
			from: start,
			to: end,
			label: formatDateRangeLabel(start, end),
			isCurrent: start.getTime() === startOfWeek(today).getTime()
		};
	}

	const start = new Date(anchor.getFullYear(), anchor.getMonth(), 1);
	const end = new Date(anchor.getFullYear(), anchor.getMonth() + 1, 0);
	return {
		from: start,
		to: end,
		label: m.period_month_label({ year: start.getFullYear(), month: start.getMonth() + 1 }),
		isCurrent: start.getTime() === new Date(today.getFullYear(), today.getMonth(), 1).getTime()
	};
}

/** 基準日を週/月モードに応じて ±1 期間ずらす。 */
export function shiftAnchor(mode: 'week' | 'month', anchor: Date, direction: -1 | 1): Date {
	if (mode === 'week') {
		return new Date(anchor.getFullYear(), anchor.getMonth(), anchor.getDate() + 7 * direction);
	}
	// 日=1 に固定してから月を加算する (例: 1/31 に +1 すると 3/3 にズレる問題を避けるため)。
	return new Date(anchor.getFullYear(), anchor.getMonth() + direction, 1);
}

/** `from`/`to` (YYYY-MM-DD) がちょうど1週 (日曜始まり) か1か月なら、週/月ナビゲーションの
 * モードと基準日を返す。どちらでもなければ `null` (任意期間)。
 *
 * 期間が今日を含むなら基準日は今日にする。`from` にすると、月をまたぐ今週から月タブへ
 * 切り替えたときに、今日を含む月ではなく前の月を表示してしまう。 */
export function detectPeriodMode(
	from: string,
	to: string,
	today: Date
): { mode: 'week' | 'month'; anchor: Date } | null {
	if (!from || !to) return null;
	const start = parseLocalDate(from);
	for (const mode of ['week', 'month'] as const) {
		const period = periodFor(mode, start, today);
		if (formatDateOnly(period.from) === from && formatDateOnly(period.to) === to) {
			return { mode, anchor: period.isCurrent ? today : start };
		}
	}
	return null;
}

/** 日付の範囲 `from`〜`to` (YYYY-MM-DD) を収める一番短い週/月。どちらにも収まらなければ範囲そのもの。 */
export function periodCovering(
	from: string,
	to: string,
	today: Date
): { from: string; to: string } {
	for (const mode of ['week', 'month'] as const) {
		const period = periodFor(mode, parseLocalDate(from), today);
		if (to <= formatDateOnly(period.to)) {
			return { from: formatDateOnly(period.from), to: formatDateOnly(period.to) };
		}
	}
	return { from, to };
}

/** 日付 `date` (YYYY-MM-DD) が表示中の期間 `from`〜`to` (空文字は指定なし) の外なら、その日を含む
 * 期間を返す。週/月の表示中ならその単位で、任意期間なら週で。期間の中なら `null`。
 * 登録した記録が一覧に見えず「消えた」と思われないよう、その記録の期間へ移すために使う。 */
export function periodToShow(
	date: string,
	from: string,
	to: string,
	today: Date
): { from: string; to: string } | null {
	if ((!from || from <= date) && (!to || date <= to)) return null;
	const mode = detectPeriodMode(from, to, today)?.mode ?? 'week';
	const period = periodFor(mode, parseLocalDate(date), today);
	return { from: formatDateOnly(period.from), to: formatDateOnly(period.to) };
}
