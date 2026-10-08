import type { ApiResult } from '$lib/api/errors';
import { DEFAULT_VISIBLE_SERIES_KEYS } from '$lib/bp-chart';
import { LatestRequest } from '$lib/latest-request';
import type { BpRecord, BpSummary } from '$lib/records';

/** 記録の一覧に一度に出す件数。全期間などで件数が多いと、全部をカードにするのが重いため
 * (3年分・約2,300件で、スマホ並みの速さだと約4秒)。 */
export const RECORD_LIST_PAGE_SIZE = 100;

type FetchRecords = (
	from: string,
	to: string
) => Promise<ApiResult<{ records: BpRecord[]; summary: BpSummary }>>;

/** 期間を切り替えて記録の一覧と集計を出す画面 (メイン画面・管理者の閲覧ページ) の状態。
 * 画面ごとに違うのは取得先だけなので、それを受け取る。 */
export class RecordsViewState {
	records = $state<BpRecord[]>([]);
	summary = $state<BpSummary | null>(null);
	loading = $state(true);
	errorMessage = $state('');
	/** 表示中の期間 (YYYY-MM-DD)。両方とも空なら全件。 */
	from = $state('');
	to = $state('');
	/** グラフに表示中の系列。 */
	visibleSeriesKeys = $state([...DEFAULT_VISIBLE_SERIES_KEYS]);
	/** 一覧に出す件数 (新しい順)。「さらに表示」で増える。期間を変えたときだけ戻し、記録を直して
	 * 取り直したときは、見ていた位置が消えないよう戻さない。 */
	listLimit = $state(RECORD_LIST_PAGE_SIZE);

	#fetchRecords: FetchRecords;
	#loadRequest = new LatestRequest();

	constructor(fetchRecords: FetchRecords) {
		this.#fetchRecords = fetchRecords;
	}

	/** 表示中の期間の一覧と集計を取得する。 */
	load = async () => {
		const isLatest = this.#loadRequest.begin();
		this.loading = true;
		this.errorMessage = '';
		const result = await this.#fetchRecords(this.from, this.to);
		if (!isLatest()) return;
		this.loading = false;
		if (!result.ok) {
			this.errorMessage = result.message;
			return;
		}
		this.records = result.records;
		this.summary = result.summary;
	};

	/** 期間ナビで期間が変わったとき。 */
	changePeriod = (from: string, to: string) => {
		this.from = from;
		this.to = to;
		this.listLimit = RECORD_LIST_PAGE_SIZE;
		return this.load();
	};

	/** `date` (`YYYY-MM-DD`) の記録がすべて一覧に出るまで、出す件数を100件単位で広げる。足した・直した
	 * 記録が「さらに表示」の後ろに隠れると、消えたと思って入れ直してしまうため。 */
	revealDate = (date: string) => {
		// 新しい順なので、その日の最後 (いちばん古い) の位置を探す。
		const last = this.records.findLastIndex((record) => record.localMeasuredAt.startsWith(date));
		if (last < this.listLimit) return;
		this.listLimit = Math.ceil((last + 1) / RECORD_LIST_PAGE_SIZE) * RECORD_LIST_PAGE_SIZE;
	};
}
