import { apiFetch } from '$lib/api/client';
import { type ApiResult, errorMessage, GENERIC_ERROR_MESSAGE } from '$lib/api/errors';
import { saveFile } from '$lib/native-app';

/** サーバーの `Content-Disposition` (src/api/records.rs) から名前が取れなかったときのファイル名。 */
const FALLBACK_FILENAME = 'bp-records.csv';

/** `Content-Disposition` の `filename="..."`。サーバーが付ける日付入りの名前で保存するため。 */
function filenameFrom(contentDisposition: string | null): string {
	return contentDisposition?.match(/filename="([^"]+)"/)?.[1] ?? FALLBACK_FILENAME;
}

/** 全件の血圧記録をエクスポートし、ファイルとして保存させる。保存したファイル名を返す。
 * FIX: `openapi-fetch` の `client` は JSON 以外のレスポンスボディ (CSV) を想定した型を持たない
 * ため、生の `fetch` を使う (契約自体は OpenAPI スキーマ通り)。
 * `<a href>` への直リンクにしないのは、401/5xx 等のエラー時にタブごとエラーページへ遷移して
 * しまうのを避け、既存の `errorMessage()` によるエラー表示に乗せるため。 */
export async function exportRecords(): Promise<ApiResult<{ filename: string }>> {
	try {
		const response = await apiFetch('/api/v1/records/export');
		if (!response.ok) {
			const body = await response.json().catch(() => undefined);
			return { ok: false, message: errorMessage(body) };
		}

		const filename = filenameFrom(response.headers.get('Content-Disposition'));
		await saveFile(filename, await response.blob());
		return { ok: true, filename };
	} catch {
		return { ok: false, message: GENERIC_ERROR_MESSAGE() };
	}
}
