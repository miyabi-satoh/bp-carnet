// 日付 (`YYYY-MM-DD`) の計算と、期間ナビのラベル。
// 「今週」などは実行する曜日・時刻とタイムゾーンで変わるため、ユーザーのタイムゾーンでの今日から求める。
// 暦の計算だけをするので、実行環境のタイムゾーンに左右されないよう UTC の `Date` で扱う。
// `src/lib/period.ts` と計算が重なるが、使わない。期待値を実装から独立に出すため。
import { ja } from './helpers';

function toUtcDate(date: string): Date {
	const [year, month, day] = date.split('-').map(Number);
	return new Date(Date.UTC(year, month - 1, day));
}

function formatDate(date: Date): string {
	return date.toISOString().slice(0, 10);
}

/** `timeZone` での今日。 */
export function todayIn(timeZone: string): string {
	// en-CA は `YYYY-MM-DD` で書式化される。
	return new Intl.DateTimeFormat('en-CA', {
		timeZone,
		year: 'numeric',
		month: '2-digit',
		day: '2-digit'
	}).format(new Date());
}

/** `timeZone` での今の時 (0〜23)。 */
export function hourIn(timeZone: string): number {
	return Number(
		new Intl.DateTimeFormat('en-GB', { timeZone, hour: '2-digit', hourCycle: 'h23' }).format(
			new Date()
		)
	);
}

export function addDays(date: string, days: number): string {
	const d = toUtcDate(date);
	d.setUTCDate(d.getUTCDate() + days);
	return formatDate(d);
}

/** その日を含む週 (日曜始まり) の最初の日。 */
export function startOfWeek(date: string): string {
	return addDays(date, -toUtcDate(date).getUTCDay());
}

/** その日の前の月の1日。 */
export function startOfPreviousMonth(date: string): string {
	const d = toUtcDate(date);
	return formatDate(new Date(Date.UTC(d.getUTCFullYear(), d.getUTCMonth() - 1, 1)));
}

function dateLabel(date: string, withYear: boolean): string {
	const [year, month, day] = date.split('-').map(Number);
	return withYear ? `${year}年${month}月${day}日` : `${month}月${day}日`;
}

/** 期間のラベル (例: `2026年9月6日 〜 9月12日`)。終わりの年は始まりと違うときだけ付く。 */
export function rangeLabel(from: string, to: string): string {
	return ja.period_range_label
		.replace('{from}', dateLabel(from, true))
		.replace('{to}', dateLabel(to, from.slice(0, 4) !== to.slice(0, 4)));
}

/** 印刷用レポートの見出しの期間 (例: `2026年9月6日 〜 2026年9月12日`)。終わりにも年が付く。 */
export function reportRangeLabel(from: string, to: string): string {
	return ja.period_range_label
		.replace('{from}', dateLabel(from, true))
		.replace('{to}', dateLabel(to, true));
}

/** その日を含む週のラベル。 */
export function weekLabel(date: string): string {
	const start = startOfWeek(date);
	return rangeLabel(start, addDays(start, 6));
}

/** その日を含む月のラベル (例: `2026年9月`)。 */
export function monthLabel(date: string): string {
	const [year, month] = date.split('-').map(Number);
	return ja.period_month_label.replace('{year}', String(year)).replace('{month}', String(month));
}
