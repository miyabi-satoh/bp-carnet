import { client } from '$lib/api/client';
import { type ApiResult, ensureOkRequest, unwrapRequest } from '$lib/api/errors';
import type { components } from '$lib/api/schema';

/** 一覧の1件 (朝/夜の区分付き)。 */
export type BpRecord = components['schemas']['RecordListItem'];
export type BpSummary = components['schemas']['RecordsSummaryResponse'];
/** 記録1件 (区分なし)。 */
export type RecordResponse = components['schemas']['RecordResponse'];
/** 記録の登録・更新で送る値。 */
export type CreateRecordRequest = components['schemas']['CreateRecordRequest'];

/** 期間 (ローカル日、YYYY-MM-DD。両端を含む) の記録。 */
export function fetchRecords(range: {
	from?: string;
	to?: string;
}): Promise<ApiResult<{ data: BpRecord[] }>> {
	return unwrapRequest(client.GET('/api/v1/records', { params: { query: range } }));
}

/** `from`/`to` は YYYY-MM-DD (空文字は指定なし)。一覧と集計は別のエンドポイントだが、片方だけ
 * 更新された中途半端な表示にならないよう、両方揃ったときだけ成功を返す。 */
export async function fetchRecordsWithSummary(
	from: string,
	to: string
): Promise<ApiResult<{ records: BpRecord[]; summary: BpSummary }>> {
	const query = { from: from || undefined, to: to || undefined };
	const [list, summary] = await Promise.all([
		fetchRecords(query),
		unwrapRequest(client.GET('/api/v1/records/summary', { params: { query } }))
	]);
	if (!list.ok) return list;
	if (!summary.ok) return summary;
	return { ok: true, records: list.data, summary: summary.data };
}

/** 最新の記録の日 (YYYY-MM-DD、ユーザーのタイムゾーン)。記録が1件も無ければ `null`。
 * メイン画面の初期表示で、その日を含む週を出すために使う。 */
export async function fetchLatestRecordDate(): Promise<ApiResult<{ date: string | null }>> {
	const result = await unwrapRequest(client.GET('/api/v1/records/latest-date'));
	if (!result.ok) return result;
	return { ok: true, date: result.data.date ?? null };
}

/** 直す・消す記録を指す組。`version` は画面に出した時点の版番号で、今の記録と違えばサーバーが
 * `record_conflict` で止める。 */
export type RecordRef = Pick<RecordResponse, 'id' | 'version'>;

/** 記録1件を取り直す。 */
export function fetchRecord(id: number): Promise<ApiResult<{ data: RecordResponse }>> {
	return unwrapRequest(client.GET('/api/v1/records/{id}', { params: { path: { id } } }));
}

/** 記録を登録する (`target` が `null`) か、既存の記録を更新する。 */
export function saveRecord(
	target: RecordRef | null,
	body: CreateRecordRequest
): Promise<ApiResult> {
	return ensureOkRequest(
		target === null
			? client.POST('/api/v1/records', { body })
			: client.PUT('/api/v1/records/{id}', {
					params: { path: { id: target.id } },
					body: { ...body, version: target.version }
				})
	);
}

/** 同じ日時 (分まで)・同じ値 (収縮期・拡張期・脈拍) の記録がすでにあるか。写真の読み取りで、
 * 同じ写真を2回登録しかけたのに気づかせるため。 */
export async function hasSameRecord(
	record: CreateRecordRequest
): Promise<ApiResult<{ exists: boolean }>> {
	const day = record.localMeasuredAt.slice(0, 10);
	const result = await fetchRecords({ from: day, to: day });
	if (!result.ok) return result;
	const exists = result.data.some(
		(r) =>
			r.localMeasuredAt.slice(0, 16) === record.localMeasuredAt.slice(0, 16) &&
			r.systolic === record.systolic &&
			r.diastolic === record.diastolic &&
			(r.pulse ?? null) === (record.pulse ?? null)
	);
	return { ok: true, exists };
}

/** 記録を削除する。 */
export function deleteRecord(target: RecordRef): Promise<ApiResult> {
	return ensureOkRequest(
		client.DELETE('/api/v1/records/{id}', {
			params: { path: { id: target.id }, query: { version: target.version } }
		})
	);
}
