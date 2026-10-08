import type { SplineProps } from 'layerchart';
import type { components } from '$lib/api/schema';
import type { ChartConfig } from '$lib/components/ui/chart';
import * as m from '$lib/paraglide/messages.js';
import { parseLocalDate } from '$lib/period';

type DailyAverages = components['schemas']['DailyAveragesResponse'];

/** 系列のキー。型・URLクエリの検証・既定の表示系列をここから作る (`buildChartSeries` と同じ並び順)。 */
export const CHART_SERIES_KEYS = [
	'morningSystolic',
	'morningDiastolic',
	'eveningSystolic',
	'eveningDiastolic'
] as const;

export type ChartSeriesKey = (typeof CHART_SERIES_KEYS)[number];

/** 折れ線グラフの1点。記録が無い区分は `null` にして線を途切れさせる。 */
export type ChartPoint = { date: Date } & Record<ChartSeriesKey, number | null>;

/** 朝/夜を表す色。文言と違いロケールに依存しないため定数で持つ。
 * 実際の値は `layout.css` の `--chart-1`/`--chart-2` (ライト/ダーク/印刷で切り替わる)。 */
const chartColors = {
	morning: 'var(--chart-1)',
	evening: 'var(--chart-2)'
} as const;

/** 下の血圧に使う破線パターン (上の血圧は実線)。 */
const DIASTOLIC_DASH_ARRAY = '6 4';

/** `LineChart` の `series` prop 1件分。
 *
 * `props` は各系列の `Spline` にそのまま渡るため、型は layerchart の `SplineProps` から
 * 取る (手書きで書き写すと、ライブラリ側の変更に型検査が反応しなくなる)。
 * `defined` は値が `null` の点で線を途切れさせるための判定。 */
export type ChartSeries = {
	key: ChartSeriesKey;
	label: string;
	color: string;
	/** 凡例のサンプルを破線で描くかどうか。 */
	dashed: boolean;
	props: Pick<SplineProps, 'defined' | 'stroke-dasharray'>;
};

function series(key: ChartSeriesKey, label: string, color: string, dashed: boolean): ChartSeries {
	return {
		key,
		label,
		color,
		dashed,
		props: {
			defined: (point) => point[key] !== null,
			...(dashed ? { 'stroke-dasharray': DIASTOLIC_DASH_ARRAY } : {})
		}
	};
}

/** 全4系列 (朝夜 × 上下)。凡例もこの順に並ぶ。
 *
 * ラベルをモジュール読み込み時に固定しないよう、定数ではなく関数にしてコンポーネント側で
 * 組み立てる。Paraglide の `setLocale()` は既定でリロードを伴うため、通常の切り替えなら
 * これで追従する。リロードしない `setLocale(locale, { reload: false })` を使う場合だけ、
 * ロケールを `$state` に橋渡しする必要がある (`getLocale()` は Svelte のリアクティブ状態では
 * なく `$derived` の依存にならないため)。 */
export function buildChartSeries(): ChartSeries[] {
	return [
		series('morningSystolic', m.chart_morning_systolic_label(), chartColors.morning, false),
		series('morningDiastolic', m.chart_morning_diastolic_label(), chartColors.morning, true),
		series('eveningSystolic', m.chart_evening_systolic_label(), chartColors.evening, false),
		series('eveningDiastolic', m.chart_evening_diastolic_label(), chartColors.evening, true)
	];
}

/** `keys` の系列だけを、`buildChartSeries` の並び順のまま返す。 */
export function visibleChartSeries(keys: readonly ChartSeriesKey[]): ChartSeries[] {
	return buildChartSeries().filter((s) => keys.includes(s.key));
}

/** 最初に表示する系列 (全4系列)。 */
export const DEFAULT_VISIBLE_SERIES_KEYS: readonly ChartSeriesKey[] = CHART_SERIES_KEYS;

/** 系列の表示を切り替えた結果を返す。最後の1本は非表示にしない (グラフが空の枠だけになるため)。 */
export function toggleSeriesKey(
	keys: readonly ChartSeriesKey[],
	key: ChartSeriesKey
): ChartSeriesKey[] {
	if (!keys.includes(key)) return [...keys, key];
	return keys.length > 1 ? keys.filter((k) => k !== key) : [...keys];
}

function isChartSeriesKey(value: string): value is ChartSeriesKey {
	return (CHART_SERIES_KEYS as readonly string[]).includes(value);
}

/** 表示中の系列を、印刷ページへ引き継ぐURLクエリ `series` の値 (カンマ区切り) にする。 */
export function formatSeriesParam(keys: readonly ChartSeriesKey[]): string {
	return keys.join(',');
}

/** URLクエリ `series` の値から表示する系列を読み取る。指定が無い、または有効なキーが1つも
 * 無ければ既定の系列を返す (手で書き換えたURL等で、グラフが空になるのを避けるため)。 */
export function parseSeriesParam(value: string | null): ChartSeriesKey[] {
	const keys = [...new Set((value ?? '').split(',').filter(isChartSeriesKey))];
	return keys.length > 0 ? keys : [...DEFAULT_VISIBLE_SERIES_KEYS];
}

/** `Chart.Container` に渡す設定。`series` から組み立てて、系列とラベル/色の対応が
 * 2箇所に分かれないようにする。 */
export function buildChartConfig(chartSeries: ChartSeries[]): ChartConfig {
	return Object.fromEntries(chartSeries.map(({ key, label, color }) => [key, { label, color }]));
}

/** 折れ線グラフ用データ。サマリーAPIの日別系列 (日付の昇順) をそのまま点に変換する。 */
export function buildChartData(days: DailyAverages[]): ChartPoint[] {
	return days.map((day) => ({
		date: parseLocalDate(day.date),
		morningSystolic: day.morning?.systolic ?? null,
		morningDiastolic: day.morning?.diastolic ?? null,
		eveningSystolic: day.evening?.systolic ?? null,
		eveningDiastolic: day.evening?.diastolic ?? null
	}));
}

/** 系列ごとの「値が入っている最後の点」。値が1つも無ければ `undefined`。
 * `buildChartData` は日付の昇順なので、末尾から探せば最新の点になる。 */
export function latestPointOf(
	chartData: ChartPoint[],
	key: ChartSeriesKey
): ChartPoint | undefined {
	for (let i = chartData.length - 1; i >= 0; i--) {
		if (chartData[i][key] !== null) return chartData[i];
	}
	return undefined;
}

/** グラフのY軸ドメイン (10刻みで丸め、上下に余白を持たせる)。0始まりのままだと
 * 血圧の変動幅 (数十mmHg程度) が画面のごく一部に圧縮されてしまうため、実データの
 * 最小値・最大値を基準にする。
 *
 * 対象は表示する系列だけに限る。上の血圧だけを表示しているときに、描かない下の血圧まで
 * 含めると線が上半分に寄ってしまう。値が1つも無ければ `undefined` を返す。 */
export function buildChartYDomain(
	chartData: ChartPoint[],
	chartSeries: ChartSeries[]
): [number, number] | undefined {
	const values = chartData.flatMap((point) =>
		chartSeries.map((s) => point[s.key]).filter((value): value is number => value !== null)
	);
	if (values.length === 0) return undefined;
	const min = Math.min(...values);
	const max = Math.max(...values);
	return [Math.floor((min - 10) / 10) * 10, Math.ceil((max + 10) / 10) * 10];
}
