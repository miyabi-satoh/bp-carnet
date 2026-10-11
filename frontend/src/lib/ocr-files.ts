import { client } from '$lib/api/client';
import { type ApiResult, GENERIC_ERROR_MESSAGE, unwrap, unwrapRequest } from '$lib/api/errors';
import type { TableColumns } from '$lib/import-table';
import type { OcrReading } from '$lib/ocr';

/** 他のアプリが書き出したファイルの読み取り (Gemini 連携、docs/import-export.md「他のアプリのファイル」)。
 * 送る前の同意は画面が取る (`file-read-consent-dialog.svelte`)。 */

/** 送れる PDF の大きさの上限 (`src/api/ocr_files.rs` の `DOCUMENT_MAX_UPLOAD_BYTES` と同じ)。 */
export const DOCUMENT_MAX_BYTES = 10 * 1024 * 1024;

/** PDF の先頭のバイト列 (`%PDF-`。`src/api/ocr_files.rs` の `PDF_MAGIC` と同じ)。 */
export const PDF_MAGIC = [0x25, 0x50, 0x44, 0x46, 0x2d];

/** 表の形のテキストの先頭 (`tableSample`) から、列の意味を決めてもらう。 */
export function fetchTableColumns(sample: string): Promise<ApiResult<{ data: TableColumns }>> {
	return unwrapRequest(client.POST('/api/v1/ocr/table-columns', { body: { sample } }));
}

/** PDF から、血圧の記録を読み取ってもらう。 */
export async function readDocument(file: File): Promise<ApiResult<{ data: OcrReading[] }>> {
	const formData = new FormData();
	formData.append('file', file, 'document.pdf');
	let response: Awaited<ReturnType<typeof postDocument>>;
	try {
		response = await postDocument(formData);
	} catch {
		return { ok: false, message: GENERIC_ERROR_MESSAGE() };
	}
	const result = unwrap(response);
	return result.ok ? { ok: true, data: result.data.readings } : result;
}

function postDocument(formData: FormData) {
	return client.POST('/api/v1/ocr/document', {
		// FIX: openapi-typescript は multipart のボディを JSON 的な形で型付けするため、実際に渡す
		// `FormData` とは型が一致しない (`$lib/ocr` の `postPhoto` と同じ)。
		// @ts-expect-error -- 上記の理由による。
		body: formData
	});
}
