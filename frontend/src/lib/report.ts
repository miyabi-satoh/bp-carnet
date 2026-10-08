import { resolve } from '$app/paths';
import { formatSeriesParam, parseSeriesParam, type ChartSeriesKey } from '$lib/bp-chart';
import { parseDateParam } from '$lib/period';

/** 期間 (YYYY-MM-DD、空文字は指定なし) とグラフに描く系列。 */
export type RecordsView = { from: string; to: string; series: ChartSeriesKey[] };

function viewQuery(from: string, to: string, series: readonly ChartSeriesKey[]): URLSearchParams {
	return new URLSearchParams({
		...(from ? { from } : {}),
		...(to ? { to } : {}),
		series: formatSeriesParam(series)
	});
}

/** 印刷用レポートのURL。期間 (YYYY-MM-DD) を `from`/`to`、グラフに描く系列を `series` の
 * クエリにする。期間がどちらも空なら全件が対象になる。 */
export function buildReportHref(
	from: string,
	to: string,
	series: readonly ChartSeriesKey[]
): string {
	return `${resolve('/report')}?${viewQuery(from, to, series)}`;
}

/** ADR: 印刷ページで再読み込みしても、戻ったときに復元できるよう sessionStorage に置く。 */
const HOME_VIEW_STORAGE_KEY = 'bp-carnet:home-view-before-report';

/** メイン画面から印刷ページへ行くときの期間・系列を控える。 */
export function saveHomeViewBeforeReport({ from, to, series }: RecordsView) {
	try {
		sessionStorage.setItem(HOME_VIEW_STORAGE_KEY, viewQuery(from, to, series).toString());
	} catch {
		// 保存できない環境では、戻ったときに初期表示になるだけ。
	}
}

/** `saveHomeViewBeforeReport` で控えた期間・系列を取り出して消す。控えが無ければ `null`。 */
export function takeHomeViewBeforeReport(): RecordsView | null {
	let saved: string | null;
	try {
		saved = sessionStorage.getItem(HOME_VIEW_STORAGE_KEY);
		sessionStorage.removeItem(HOME_VIEW_STORAGE_KEY);
	} catch {
		return null;
	}
	if (saved === null) return null;
	const query = new URLSearchParams(saved);
	return {
		from: parseDateParam(query.get('from')),
		to: parseDateParam(query.get('to')),
		series: parseSeriesParam(query.get('series'))
	};
}
