import { client } from '$lib/api/client';
import { type ApiResult, unwrapRequest } from '$lib/api/errors';
import type { components } from '$lib/api/schema';
import * as m from '$lib/paraglide/messages.js';
import { getLocale } from '$lib/paraglide/runtime.js';
import { parseClockTime, type DayPeriod } from '$lib/period';

export type Settings = components['schemas']['SettingsResponse'];

/** 朝・夜の時間帯 (0:00 からの経過分数)。 */
export type PeriodThresholds = Pick<
	Settings,
	'morningStartMin' | 'morningEndMin' | 'eveningStartMin' | 'eveningEndMin'
>;

/** 時刻 (`HH:MM`) が朝/夜のどちらに属するか。区間は `[開始, 終了)` で、どちらでもなければ `undefined`
 * (サーバーの集計と同じ判定)。 */
export function dayPeriodAt(time: string, thresholds: PeriodThresholds): DayPeriod | undefined {
	const minutes = parseClockTime(time);
	if (minutes === undefined) return undefined;
	if (thresholds.morningStartMin <= minutes && minutes < thresholds.morningEndMin) return 'morning';
	if (thresholds.eveningStartMin <= minutes && minutes < thresholds.eveningEndMin) return 'evening';
	return undefined;
}

/** 設定を取得できなかったときの朝/夜の時間帯 (`users` の列の既定値と同じ)。 */
export const DEFAULT_PERIOD_THRESHOLDS: PeriodThresholds = {
	morningStartMin: 240,
	morningEndMin: 600,
	eveningStartMin: 1080,
	eveningEndMin: 1440
};

/** `GET /api/v1/settings` で個人設定を取得する。 */
export function fetchSettings(): Promise<ApiResult<{ data: Settings }>> {
	return unwrapRequest(client.GET('/api/v1/settings'));
}

/** 手動で選べるタイムゾーンの選択肢 (保存の検証に通る名前だけ) を取得する。 */
export function fetchTimezones(): Promise<
	ApiResult<{ data: components['schemas']['TimezonesResponse'] }>
> {
	return unwrapRequest(client.GET('/api/v1/settings/timezones'));
}

/** 個人設定を保存し、保存後の設定を返す。 */
export function saveSettings(
	settings: components['schemas']['UpdateSettingsRequest']
): Promise<ApiResult<{ data: Settings }>> {
	return unwrapRequest(client.PUT('/api/v1/settings', { body: settings }));
}

/** 朝/夜の時間帯だけを取得する。写真OCRの仮時刻の基準のように、設定画面の外から時間帯だけが
 * 要る場面で使う。取得できなくても呼び出し側の表示は止めず、既定の時間帯で続ける。 */
export async function fetchPeriodThresholds(): Promise<PeriodThresholds> {
	const result = await fetchSettings();
	if (!result.ok) return DEFAULT_PERIOD_THRESHOLDS;
	return periodThresholdsOf(result.data);
}

/** 個人設定から朝/夜の時間帯だけを取り出す。 */
export function periodThresholdsOf(settings: Settings): PeriodThresholds {
	const { morningStartMin, morningEndMin, eveningStartMin, eveningEndMin } = settings;
	return { morningStartMin, morningEndMin, eveningStartMin, eveningEndMin };
}

/** 時間帯のエラーを、朝・夜どちらの行に出すかと合わせて返す。規則はバックエンド
 * (`validation::validate_period_thresholds`) と同じ。欄は選択式で値域外の値が入らないため、値域は見ない。
 *
 * 朝・夜の重なりは、最後に変えた側の行に出す (両方の行に同じ文言を並べないため)。 */
export function periodThresholdErrors(
	thresholds: PeriodThresholds,
	lastEdited: DayPeriod
): Partial<Record<DayPeriod, string>> {
	const { morningStartMin, morningEndMin, eveningStartMin, eveningEndMin } = thresholds;
	const errors: Partial<Record<DayPeriod, string>> = {};
	if (morningStartMin >= morningEndMin) errors.morning = m.settings_period_not_ascending_error();
	if (eveningStartMin >= eveningEndMin) errors.evening = m.settings_period_not_ascending_error();
	if (errors.morning !== undefined || errors.evening !== undefined) return errors;
	// 区間は終了側を含まないため、境界が接するだけ (朝の終了 == 夜の開始) は重複としない。
	if (morningStartMin < eveningEndMin && eveningStartMin < morningEndMin) {
		errors[lastEdited] = m.settings_period_overlapping_error();
	}
	return errors;
}

/** ゾーンの `timeZoneName` の部分。閲覧環境が知らないゾーン (RangeError) は `undefined`。 */
function timeZoneNamePart(
	locale: string,
	zone: string,
	style: 'longGeneric' | 'longOffset',
	now: Date
): string | undefined {
	try {
		return new Intl.DateTimeFormat(locale, { timeZone: zone, timeZoneName: style })
			.formatToParts(now)
			.find((part) => part.type === 'timeZoneName')?.value;
	} catch {
		return undefined;
	}
}

/** タイムゾーンの表示言語での表示名 (例: `日本標準時`)。`longGeneric` で引く (`long` は夏時間の期間かどうかで表記が変わるため)。
 *
 * `Etc/UTC` のように表示名を持たないゾーンは `GMT+00:00` のような時差そのものが返る。時差は選択肢の
 * 先頭に別に出しているため、これは表示名として扱わない。 */
function timezoneName(zone: string, now = new Date()): string | undefined {
	const name = timeZoneNamePart(getLocale(), zone, 'longGeneric', now);
	return name === undefined || /^GMT([+-]|$)/.test(name) ? undefined : name;
}

/** 今の UTC からの時差 (分)。引けなければ `undefined`。 */
function utcOffsetMinutes(zone: string, now: Date): number | undefined {
	// `en-US` の `longOffset` は `GMT+09:00`、時差が無ければ `GMT`。
	const part = timeZoneNamePart('en-US', zone, 'longOffset', now);
	const match = /^GMT(?:([+-])(\d{2}):(\d{2}))?$/.exec(part ?? '');
	if (!match) return undefined;
	if (!match[1]) return 0;
	const minutes = Number(match[2]) * 60 + Number(match[3]);
	return match[1] === '-' ? -minutes : minutes;
}

function formatUtcOffset(minutes: number): string {
	const sign = minutes < 0 ? '-' : '+';
	const abs = Math.abs(minutes);
	return `UTC${sign}${String(Math.floor(abs / 60)).padStart(2, '0')}:${String(abs % 60).padStart(2, '0')}`;
}

/** タイムゾーンの表示 (例: `Asia/Tokyo (日本標準時)`)。表示名を引けないゾーン (閲覧環境の
 * ICU データに無い等) は IANA 名だけにする。 */
export function timezoneLabel(zone: string): string {
	const name = timezoneName(zone);
	return name ? `${zone} (${name})` : zone;
}

/** 設定画面の「自動設定」の選択肢の表示 (例: `自動設定 (Asia/Tokyo 日本標準時)`)。 */
export function autoTimezoneLabel(zone: string): string {
	const name = timezoneName(zone);
	return m.settings_timezone_auto_option({ zone: name ? `${zone} ${name}` : zone });
}

export type TimezoneOption = { zone: string; label: string };

/** 手動の選択肢を、今の時差の順に「(UTC+09:00) 日本標準時」の形で並べる。同じ表示名が並ぶゾーン
 * (例: パリとベルリンの「中央ヨーロッパ時間」) には都市名を添えて見分けられるようにする。 */
export function timezoneChoiceOptions(
	zones: readonly string[],
	now = new Date()
): TimezoneOption[] {
	const items = zones.map((zone) => ({
		zone,
		offset: utcOffsetMinutes(zone, now),
		name: timezoneName(zone, now) ?? zone
	}));
	const nameCounts = new Map<string, number>();
	for (const item of items) nameCounts.set(item.name, (nameCounts.get(item.name) ?? 0) + 1);
	return items
		.sort(
			(a, b) =>
				(a.offset ?? 0) - (b.offset ?? 0) ||
				a.name.localeCompare(b.name, getLocale()) ||
				a.zone.localeCompare(b.zone, getLocale())
		)
		.map(({ zone, offset, name }) => {
			const city = zone.split('/').at(-1)?.replaceAll('_', ' ') ?? zone;
			const suffix = (nameCounts.get(name) ?? 0) > 1 ? ` (${city})` : '';
			const prefix = offset === undefined ? '' : `(${formatUtcOffset(offset)}) `;
			return { zone, label: `${prefix}${name}${suffix}` };
		});
}

/** ブラウザのタイムゾーン (IANA 名)。取れなければ `undefined`。 */
export function browserTimeZone(): string | undefined {
	try {
		return Intl.DateTimeFormat().resolvedOptions().timeZone || undefined;
	} catch {
		return undefined;
	}
}

/** 自動設定のユーザーのタイムゾーンを、ブラウザのタイムゾーンに揃える。手動設定ならサーバーは変えない。 */
export function saveDetectedTimezone(timezone: string): Promise<ApiResult<{ data: Settings }>> {
	return unwrapRequest(client.PUT('/api/v1/settings/detected-timezone', { body: { timezone } }));
}
