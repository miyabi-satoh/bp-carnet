import type { components } from '$lib/api/schema';
import {
	fillPlaceholderTimes,
	importRowStatus,
	parseCsvTable,
	settleNewRows,
	type ImportRow
} from '$lib/import';
import { isValidDateTime, toLocalDateTime } from '$lib/period';
import { splitMeasuredAt } from '$lib/record-form';
import type { PeriodThresholds } from '$lib/settings';

/** 他のアプリが書き出した表の形のテキスト (CSV など) の、列の意味。Gemini が先頭の数行から決める
 * (`POST /ocr/table-columns`、docs/import-export.md「他のアプリのファイル」)。 */
export type TableColumns = components['schemas']['TableColumns'];
type TableColumnGroup = components['schemas']['TableColumnGroup'];
type DateOrder = components['schemas']['DateOrder'];

/** 列を決めてもらうために送る行数。見出しと、日付の並び (月と日のどちらが先か) を値から決められる数行。
 * 記録そのものを送る量を抑えるため、これより多くは送らない。 */
export const SAMPLE_LINES = 12;
/** 送る1行の長さの上限。長いメモの列があっても、列を決めるのに要るのは先頭だけ。 */
const SAMPLE_LINE_MAX_CHARS = 400;

const DELIMITERS: Record<TableColumns['delimiter'], string> = {
	comma: ',',
	semicolon: ';',
	tab: '\t'
};

/** 空行を除いた行。見本の行数 (`headerLines`) と、表に分けた行 (`parseCsvTable` は空行を読み飛ばす) の
 * 数え方をそろえる。空白だけの行は `parseCsvTable` が行として残すので、ここでも除かない。 */
function nonEmptyLines(text: string): string[] {
	return text
		.replace(/^\uFEFF/, '')
		.split(/\r\n|\r|\n/)
		.filter((line) => line !== '');
}

/** 列を決めてもらうために Gemini へ送る、ファイルの先頭の数行。 */
export function tableSample(text: string): string {
	return nonEmptyLines(text)
		.slice(0, SAMPLE_LINES)
		.map((line) => line.slice(0, SAMPLE_LINE_MAX_CHARS))
		.join('\n');
}

const MONTH_NAMES = [
	'jan',
	'feb',
	'mar',
	'apr',
	'may',
	'jun',
	'jul',
	'aug',
	'sep',
	'oct',
	'nov',
	'dec'
];

function monthFromName(name: string): number | undefined {
	const index = MONTH_NAMES.indexOf(name.slice(0, 3).toLowerCase());
	return index < 0 ? undefined : index + 1;
}

type DateParts = { year: number; month: number; day: number; rest: string };

/** 文字列の先頭の日付を読み、残り (時刻が続いていればその部分) と一緒に返す。 */
function leadingDate(text: string, order: DateOrder): DateParts | null {
	const numeric = (year: string, month: string, day: string, rest: string): DateParts => ({
		year: Number(year),
		month: Number(month),
		day: Number(day),
		rest
	});
	// 年が先 (`2026-02-05`・`2026/2/5`・`2026.2.5`・`2026年2月5日`・`20260205`)。年が4桁で先頭なら、並びの指定によらない。
	let match =
		/^(\d{4})\s*[-/.年]\s*(\d{1,2})\s*[-/.月]\s*(\d{1,2})\s*日?/.exec(text) ??
		/^(\d{4})(\d{2})(\d{2})(?!\d)/.exec(text);
	if (match) return numeric(match[1], match[2], match[3], text.slice(match[0].length));

	// 年が後ろ (`02/05/2026`・`05.02.2026`)。月と日のどちらが先かは、値からは決まらないことがある。
	match = /^(\d{1,2})\s*[-/.]\s*(\d{1,2})\s*[-/.]\s*(\d{4})/.exec(text);
	if (match) {
		const [first, second] = [Number(match[1]), Number(match[2])];
		// 13 以上の数は月でないので、指定と食い違っていても値に従う。
		const dayFirst = first > 12 || (second <= 12 && order === 'dmy');
		const [month, day] = dayFirst ? [second, first] : [first, second];
		return { year: Number(match[3]), month, day, rest: text.slice(match[0].length) };
	}

	// 月が名前 (`Sep 28 2026`・`September 28, 2026`・`28 Sep 2026`)。
	match = /^([A-Za-z]{3,9})\.?\s+(\d{1,2}),?\s+(\d{4})/.exec(text);
	if (match) {
		const month = monthFromName(match[1]);
		if (month === undefined) return null;
		return {
			year: Number(match[3]),
			month,
			day: Number(match[2]),
			rest: text.slice(match[0].length)
		};
	}
	match = /^(\d{1,2})\s+([A-Za-z]{3,9})\.?,?\s+(\d{4})/.exec(text);
	if (match) {
		const month = monthFromName(match[2]);
		if (month === undefined) return null;
		return {
			year: Number(match[3]),
			month,
			day: Number(match[1]),
			rest: text.slice(match[0].length)
		};
	}
	return null;
}

/** 時刻 (`7:08`・`07:08:30`・`7:08 PM`・`午後7:08`・`7時8分`) を、24時間の時・分にする。
 * 時刻が書かれていなければ `undefined`、書かれているが読めなければ `null`。秒は捨てる。 */
function clockTime(text: string): { hour: number; minute: number } | null | undefined {
	const trimmed = text.trim();
	if (trimmed === '') return undefined;
	const match =
		/^(午前|午後|AM|PM)?\s*(\d{1,2})\s*[:時]\s*(\d{1,2})\s*(?:[:分]\s*\d{1,2}(?:\.\d+)?)?\s*[分秒]?\s*(AM|PM)?$/i.exec(
			trimmed
		);
	if (!match) return null;
	const meridiem = (match[1] ?? match[4])?.toUpperCase();
	let hour = Number(match[2]);
	const minute = Number(match[3]);
	if (meridiem !== undefined) {
		if (hour > 12) return null;
		const afternoon = meridiem === 'PM' || meridiem === '午後';
		// 12時間表記の 12 時は、午前なら 0 時、午後なら 12 時。
		hour = (hour % 12) + (afternoon ? 12 : 0);
	}
	return { hour, minute };
}

/** オフセット付きの日時 (`2026-02-05T07:08:00+09:00`・`2026-02-05 07:08:00 +0900`・末尾が `Z`) を瞬間にする。
 * オフセットが無ければ `null`。 */
export function instantWithOffset(text: string): Date | null {
	const match =
		/^(\d{4}-\d{2}-\d{2})[T ](\d{2}:\d{2}(?::\d{2}(?:\.\d+)?)?)\s*(Z|[+-]\d{2}:?\d{2})$/i.exec(
			text
		);
	if (!match) return null;
	const offset =
		match[3].toUpperCase() === 'Z' ? 'Z' : match[3].replace(/^([+-]\d{2}):?(\d{2})$/, '$1:$2');
	const date = new Date(`${match[1]}T${match[2]}${offset}`);
	return Number.isNaN(date.getTime()) ? null : date;
}

/** 他のアプリの日付と時刻の欄を、個人設定のタイムゾーンでの日付・時刻の2欄にする。
 * - 時刻が無ければ時刻を空で返す (仮の時刻は後で振る、`fillPlaceholderTimes`)。
 * - 秒は捨てる。他のアプリは秒まで書くものが多く、秒のある行を読めない日時にすると全行が取り込めなくなるため
 *   (このアプリが書き出した CSV は秒を捨てない、`$lib/import` の `csvMeasuredAt`)。
 * - 読めなければ `null`。 */
export function tableMeasuredAt(
	dateCell: string,
	timeCell: string | undefined,
	order: DateOrder,
	utc: boolean,
	timeZone: string
): { measuredOnDate: string; time: string } | null {
	// 全角の数字・記号を半角にそろえ、曜日などの括弧書き (`2026/02/05(木)`) を除く。
	const normalize = (cell: string) =>
		cell
			.normalize('NFKC')
			.replace(/\([^)]*\)/g, ' ')
			.trim();
	const dateText = normalize(dateCell);
	const timeText = timeCell === undefined ? '' : normalize(timeCell);

	const instant = instantWithOffset(timeText === '' ? dateText : `${dateText} ${timeText}`);
	if (instant) return splitMeasuredAt(toLocalDateTime(instant, timeZone));

	const date = leadingDate(dateText, order);
	if (!date) return null;
	// 日付と時刻の間の `T`・`,` (`2026-02-05T07:08`・`Sep 28 2026, 07:45`) は区切り。
	const time = clockTime(timeText === '' ? date.rest.replace(/^\s*[T,]?\s*/i, '') : timeText);
	if (time === null) return null;

	const pad = (value: number) => String(value).padStart(2, '0');
	const measuredOnDate = `${String(date.year).padStart(4, '0')}-${pad(date.month)}-${pad(date.day)}`;
	if (time === undefined) {
		return isValidDateTime(date.year, date.month, date.day) ? { measuredOnDate, time: '' } : null;
	}
	if (!isValidDateTime(date.year, date.month, date.day, time.hour, time.minute)) return null;
	if (utc) {
		const utcInstant = new Date(
			Date.UTC(date.year, date.month - 1, date.day, time.hour, time.minute)
		);
		return splitMeasuredAt(toLocalDateTime(utcInstant, timeZone));
	}
	return { measuredOnDate, time: `${pad(time.hour)}:${pad(time.minute)}` };
}

/** 数値の欄を、記録フォームが読む形 (10進の整数) にそろえる。`120.0` の小数点以下の 0 と、
 * 後ろに付いた単位 (`120 mmHg`) を除く。それ以外 (`120.5` など) はそのまま返し、行のエラーとして直してもらう。 */
/** 脈拍の欄。測っていないことを `0` や `-` で書くアプリがあるので、未入力として読む
 * (そのままだと、脈拍を測らなかった行がすべてエラーになる)。 */
function tablePulse(cell: string): string {
	const value = tableNumber(cell);
	return /^(0+|[-‐－ー—]+)$/.test(value) ? '' : value;
}

function tableNumber(cell: string): string {
	const trimmed = cell.normalize('NFKC').trim();
	const match = /^(\d+)(?:\.0+)?\s*(?:mmHg|bpm|拍\/分|回\/分)?$/i.exec(trimmed);
	return match ? match[1] : trimmed;
}

function rowFromGroup(
	cols: string[],
	group: TableColumnGroup,
	columns: TableColumns,
	timeZone: string
): ImportRow | null {
	const cell = (index: number | null) => (index === null ? '' : (cols[index] ?? ''));
	const systolic = tableNumber(cell(group.systolic));
	const diastolic = tableNumber(cell(group.diastolic));
	// 朝と夜が横に並ぶ表で、測っていない回の欄は空。記録にしない。
	if (systolic === '' && diastolic === '') return null;

	const measuredAt = tableMeasuredAt(
		cell(group.date),
		group.time === null ? undefined : cell(group.time),
		columns.dateOrder,
		columns.utc,
		timeZone
	);
	return {
		keep: true,
		// 読めない日時は空にする。日時の欄も空で出るので、記録フォームと同じ「入力してください」になる。
		measuredOnDate: measuredAt?.measuredOnDate ?? '',
		time: measuredAt?.time ?? '',
		timeIsPlaceholder: measuredAt !== null && measuredAt.time === '',
		periodHint: group.period ?? undefined,
		systolic,
		diastolic,
		pulse: tablePulse(cell(group.pulse)),
		memo: cell(group.memo).trim()
	};
}

/** 他のアプリが書き出した表の形のテキストを、`columns` の列の意味で取り込みの行にする。
 * 行の検証・仮の時刻・チェックの初期状態は、このアプリの CSV と同じ (`$lib/import`)。 */
export function parseTableImport(
	text: string,
	columns: TableColumns,
	timeZone: string,
	periods: PeriodThresholds
): ImportRow[] {
	const table = parseCsvTable(text.replace(/^\uFEFF/, ''), DELIMITERS[columns.delimiter]);
	const rows = table.slice(columns.headerLines).flatMap((cols) => {
		// 朝と夜が横に並ぶ表のメモの列は1日に1つで、測定ごとの列ではない。同じ列を指す測定が複数あれば、
		// その行の最初の記録にだけメモを付ける (同じメモを朝と夜の両方に付けない)。
		const usedMemoColumns = new Set<number>();
		return columns.groups.flatMap((group) => {
			const row = rowFromGroup(cols, group, columns, timeZone);
			if (!row) return [];
			if (group.memo !== null) {
				if (usedMemoColumns.has(group.memo)) row.memo = '';
				usedMemoColumns.add(group.memo);
			}
			return row;
		});
	});
	fillPlaceholderTimes(rows, periods);
	settleNewRows(rows, importRowStatus);
	return rows;
}
