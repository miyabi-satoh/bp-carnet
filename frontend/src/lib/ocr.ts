import { resolve } from '$app/paths';
import { client } from '$lib/api/client';
import { createFileHandoff } from '$lib/file-handoff';
import {
	type ApiFailure,
	type ApiResult,
	ensureOkRequest,
	errorCode,
	GENERIC_ERROR_MESSAGE,
	unwrap,
	unwrapRequest
} from '$lib/api/errors';
import type { components } from '$lib/api/schema';
import {
	fillPlaceholderTimes,
	importRowStatus,
	revalidateRow,
	settleNewRows,
	type ImportEntry,
	type ImportRow,
	type ImportRowStatus
} from '$lib/import';
import { isNativeApp } from '$lib/native-app';
import * as m from '$lib/paraglide/messages.js';
import { formatDateOnly, isValidDateTime } from '$lib/period';
import { resizeImage } from '$lib/ocr-image';
import { dayPeriodAt, type PeriodThresholds } from '$lib/settings';
import { parseStrictInt } from '$lib/utils';

export type OcrResult = components['schemas']['OcrResult'];
export type OcrReading = components['schemas']['OcrReading'];

export type OcrQuota = components['schemas']['OcrQuotaResponse'];

export type OcrStatus = {
	/** 写真の読み取りが使えるか。取得できなければ `null` (分からない)。 */
	enabled: boolean | null;
	/** 読み取りの枠を使う順に並べたもの。無制限・無効 (または取得できない) なら `null` で、画面には出さない。 */
	quotas: OcrQuota[] | null;
	/** 全部の枠を足した残りが、買い足し1回分の20%以下か (使い切りを含む)。 */
	quotaLow: boolean;
	/** 読み取りを買い足せるか (無料の枠に上限があり、ウェブは Stripe、アプリはアプリ内課金が有効)。 */
	topupAvailable: boolean;
	/** 写真を Gemini API に送ることに同意しているか。取得できなければ `null` (分からない)。 */
	consented: boolean | null;
};

/** 全部の枠を使い切ったか。無制限 (`null`) なら `false`。 */
export function quotasExhausted(quotas: OcrQuota[] | null): boolean {
	return quotas !== null && quotas.every((quota) => quota.remainingPercent === 0);
}

/** 写真を選ぶ `<input type="file">` の `accept`。撮影もできるかは OS に任せ、`capture` は付けない。 */
export const PHOTO_FILE_ACCEPT = 'image/*';

/** ホーム画面で選んだ、写真で記録のページへ渡す写真。 */
export const photoFileHandoff = createFileHandoff(resolve('/record/photo'));

/** `GET /api/v1/ocr/status` — 未ログイン以外は失敗させない。取得できなければ、有効かは `null`
 * (分からない)、買い足しは出さない扱いにする (通信エラー等で誤って買い足しの導線を出さないため)。
 * 無効なら、枠の残りも買い足しも意味を持たないので、出さない値にそろえる (サーバーは無効でも枠を返す)。 */
export async function fetchOcrStatus(): Promise<OcrStatus> {
	const result = await unwrapRequest(client.GET('/api/v1/ocr/status'));
	if (!result.ok)
		return {
			enabled: null,
			quotas: null,
			quotaLow: false,
			topupAvailable: false,
			consented: null
		};
	if (!result.data.enabled)
		return {
			enabled: false,
			quotas: null,
			quotaLow: false,
			topupAvailable: false,
			consented: result.data.consented
		};
	return {
		enabled: result.data.enabled,
		quotas: result.data.quotas ?? null,
		quotaLow: result.data.quotaLow,
		// アプリはアプリ内課金で売る (docs/payments.md)。`topupAvailable` は Stripe のもの。
		topupAvailable: isNativeApp ? result.data.appStoreTopupAvailable : result.data.topupAvailable,
		consented: result.data.consented
	};
}

/** 写真を Gemini API に送ることに同意する (docs/ocr.md)。 */
export function consentToOcr(): Promise<ApiResult> {
	return ensureOkRequest(client.POST('/api/v1/ocr/consent'));
}

/** 写真を Gemini API に送ることへの同意を取り消す。 */
export function withdrawOcrConsent(): Promise<ApiResult> {
	return ensureOkRequest(client.DELETE('/api/v1/ocr/consent'));
}

/** 写真の読み取りの失敗。`blocked` は、写真を選び直しても読み取れない失敗
 * (枠を使い切った、読み取りが無効)。このとき画面は「もう一度」を出さない (docs/mobile-app.md)。 */
export type PhotoReadFailure = ApiFailure & { blocked: boolean };

/** 写真を Gemini に送って血圧計液晶の読み取りを依頼する。アップロード前にブラウザ側で
 * リサイズする (`resizeImage`)。リサイズの失敗も通信エラーと同じ汎用文言の失敗にする。 */
export async function extractFromPhoto(
	file: File
): Promise<{ ok: true; data: OcrResult } | PhotoReadFailure> {
	let response: Awaited<ReturnType<typeof postPhoto>>;
	try {
		response = await postPhoto(file);
	} catch {
		return { ok: false, message: GENERIC_ERROR_MESSAGE(), blocked: false };
	}
	const result = unwrap(response);
	if (result.ok) return result;
	return { ...result, blocked: blocksReading(response.error) };
}

function blocksReading(body: unknown): boolean {
	switch (errorCode(body)) {
		case 'ocr_budget_exhausted':
		case 'ocr_disabled':
			return true;
		default:
			return false;
	}
}

async function postPhoto(file: File) {
	const resized = await resizeImage(file);
	const formData = new FormData();
	formData.append('image', resized, 'photo.jpg');

	// openapi-fetch は body が `FormData` のとき Content-Type を自身では設定しない
	// (ブラウザに boundary 付きの `multipart/form-data` を任せる)。
	return client.POST('/api/v1/ocr', {
		// FIX: openapi-typescript は multipart のボディを `{ image: string (binary) }` という
		// JSON 的な形で型付けするため、実際に渡す `FormData` とは型が一致しない
		// (openapi-typescript 側の型表現の限界であり、ランタイムの契約自体は OpenAPI スキーマ通り)。
		// @ts-expect-error -- 上記の理由による。
		body: formData
	});
}

/** 「まず疑う」の境界となる信頼度 (信頼度「中」の下限)。信頼度の3段階の表示と、複数行読み取りの
 * 「要確認」・初期状態のチェック (`memoRowsFromReadings`) で同じ値を使うため定数にしている。 */
const LOW_CONFIDENCE_THRESHOLD = 0.75;

export type ConfidenceLevel = 'high' | 'medium' | 'low';

/** 信頼度 (0.0-1.0) の3段階 (docs/ocr.md)。 */
export function confidenceLevel(confidence: number): ConfidenceLevel {
	if (confidence >= 0.9) return 'high';
	if (confidence >= LOW_CONFIDENCE_THRESHOLD) return 'medium';
	return 'low';
}

/** 手書きメモの複数行読み取り結果 1 行分。CSV の行 (`ImportRow`) に読み取り固有のものを足した
 * 形で、取り込みの一覧・確認・送信は CSV と同じ道を通る (`$lib/import`)。 */
export type MemoRow = ImportRow & {
	confidence: number;
	/** Gemini からの補足 (判読できない字など)。記録には取り込まない。 */
	note: string;
	/** アプリが決めた日付 (年の見当・「年」の欄)。`measuredOnDate` がこれと同じ行だけを「年」の欄で動かす
	 * (本人がシートで入れた日付は動かさない)。日付を読み取れなかった行は空。 */
	inferredDate: string;
};

/** 手書きメモの行の状態。CSV と同じ判定に、読み取りの自信の低さを足す (docs/import-export.md)。 */
export function memoRowStatus(row: MemoRow): ImportRowStatus {
	const status = importRowStatus(row);
	if (status !== 'ok') return status;
	return row.confidence < LOW_CONFIDENCE_THRESHOLD ? 'warn' : 'ok';
}

/** 手書きメモの行に添える「何をすればよいか」の文。仮の時刻を振ったことはここでは伝えない
 * (全行に並んで邪魔になるため。シートを開いたときだけ出す、docs/ocr.md)。 */
export function memoRowNote(row: MemoRow): string | undefined {
	if (row.measuredOnDate === '') return m.import_rows_note_date_unread();
	if (row.rowError) return row.rowError;
	if (row.confidence < LOW_CONFIDENCE_THRESHOLD) return m.import_rows_note_low_confidence();
	return undefined;
}

/** 手書きメモの "MM-DD" を、今日 (個人設定のタイムゾーンでの日付) を基準に年を補完した "YYYY-MM-DD" に変換する。
 * 手書きメモには年が書かれないことが多いため今年と仮定するが、それだと未来日になって
 * しまう場合 (例: 昨年12月分のメモを年明けに読み込んだ) は 1 年前だったとみなす。
 * 古い手帳なら、一覧の「年」の欄で直してもらう (`shiftMemoRowsYear`、docs/ocr.md)。
 * "MM-DD" として解釈できない場合、または今年・去年のどちらとしても実在しない日付
 * (OCR誤読による "02-31" 等) の場合は空にする。行は「要確認」になり、日付を入れてもらう
 * (今日にすると、読めなかったことに気づかないまま今日の記録として取り込まれるため)。 */
export function inferMeasuredOnYear(measuredOn: string | null | undefined, today: Date): string {
	const match = /^(\d{2})-(\d{2})$/.exec(measuredOn ?? '');
	if (!match) return '';

	const month = Number(match[1]);
	const day = Number(match[2]);
	const dateIn = (year: number) =>
		isValidDateTime(year, month, day) ? new Date(year, month - 1, day) : null;
	const thisYear = dateIn(today.getFullYear());

	if (thisYear && thisYear.getTime() <= today.getTime()) return formatDateOnly(thisYear);

	// 今年だと未来日になる (または今年としては実在しない) 場合、去年だったとみなす。
	const lastYear = dateIn(today.getFullYear() - 1);
	if (lastYear) return formatDateOnly(lastYear);

	// 去年としても実在しない場合、今年の値がまだ使えるならそれを使う (例: うるう年の
	// "02-29" を去年の同日として読んだが去年はうるう年でなかった場合)。それも無理なら
	// (今年・去年のどちらとしても実在しない、真に不正な "MM-DD") 空にする。
	return thisYear ? formatDateOnly(thisYear) : '';
}

/** 日付をアプリが決めたままの行 (本人がシートで日付を直していない行)。 */
function hasInferredDate(row: MemoRow): boolean {
	return row.inferredDate !== '' && row.measuredOnDate === row.inferredDate;
}

/** 手書きメモの行の年 (一覧の「年」の欄の値)。年の変わり目をまたぐページは新しいほうの年にする。
 * アプリが決めた日付の行が無ければ `null`。 */
export function memoRowsYear(rows: readonly MemoRow[]): number | null {
	let year: number | null = null;
	for (const row of rows) {
		if (hasInferredDate(row)) year = Math.max(year ?? 0, Number(row.measuredOnDate.slice(0, 4)));
	}
	return year;
}

/** 手書きメモの行の年を、「年」の欄で選んだ年に合わせてずらす (docs/ocr.md)。1ページの行は続いた日付なので、
 * アプリが決めた日付の行をすべて同じ年数だけずらす (12月から1月へまたぐページも、またいだまま移る)。
 * ずらした先に無い日付 (うるう日) は、そのまま出して「要確認」にする。 */
export function shiftMemoRowsYear(rows: MemoRow[], year: number): void {
	const current = memoRowsYear(rows);
	if (current === null || current === year) return;
	const delta = year - current;
	for (const row of rows) {
		if (!hasInferredDate(row)) continue;
		const shifted = Number(row.measuredOnDate.slice(0, 4)) + delta;
		row.measuredOnDate = `${String(shifted).padStart(4, '0')}${row.measuredOnDate.slice(4)}`;
		row.inferredDate = row.measuredOnDate;
		revalidateRow(row);
	}
}

/** 読み取り結果の脈拍を入力欄の値にする。液晶・メモに脈拍の記載が無いと Gemini は 0 を返す
 * (スキーマ上 null にできないため) が、そのまま出すと「脈拍0」と読み取れたように見えるため
 * 未入力扱いにする。 */
export function pulseFromReading(pulse: number): number | undefined {
	return pulse > 0 ? pulse : undefined;
}

/** Gemini の読み取り結果 (`OcrResult.readings`) を、編集可能な `MemoRow[]` に変換する。
 * - 信頼度が `LOW_CONFIDENCE_THRESHOLD` 未満の行は初期状態でチェックを外す
 *   (誤読を誤って一括登録しないため)
 * - 脈拍 0 は未入力扱いにする (`pulseFromReading`)
 * - 時刻が読み取れなかった行には、朝/夜の時間帯に仮時刻を振る (`fillPlaceholderTimes`。行に書かれた朝・夜を優先する)。ユーザーが
 *   インポート前に必ず自分で確認・修正できるよう `<input type="time">` は編集可能にする */
export function memoRowsFromReadings(
	readings: OcrReading[],
	periods: PeriodThresholds,
	today: Date
): MemoRow[] {
	const rows = readings.map((r): MemoRow => {
		const measuredOnDate = inferMeasuredOnYear(r.measuredOn, today);
		return {
			keep: true,
			measuredOnDate,
			inferredDate: measuredOnDate,
			time: r.time?.trim() ?? '',
			// 日付が無い行には仮の時刻を振らない (`fillPlaceholderTimes`)。
			timeIsPlaceholder: !r.time?.trim() && measuredOnDate !== '',
			periodHint: r.period ?? undefined,
			systolic: String(r.systolic),
			diastolic: String(r.diastolic),
			pulse: String(pulseFromReading(r.pulse) ?? ''),
			confidence: r.confidence,
			note: r.note ?? '',
			memo: r.memo ?? ''
		};
	});
	fillPlaceholderTimes(rows, periods);
	settleNewRows(rows, memoRowStatus);
	return rows;
}

/** 読み直しで置き換えられる既存の記録から、メモを行に引き継ぐ (docs/ocr.md)。
 * 同じ日・同じ朝/夜の枠で収縮期・拡張期が一致する記録があれば、そのメモで行のメモを上書きする
 * (手帳から読み取った文章より、画面から書いたメモを優先する)。記録1件は1行にだけ引き継ぎ、
 * 時刻まで一致する記録を先に対応づける。メモの無い記録は対象にしない。
 * `canCarry` が `false` の行 (取得を待つ間にメモが編集された行など) には引き継がない。 */
export function carryOverMemos(
	rows: MemoRow[],
	existing: ImportEntry[],
	periods: PeriodThresholds,
	canCarry: (row: MemoRow) => boolean = () => true
): void {
	const remaining = existing.filter((entry) => entry.memo !== '');
	const matchIndex = (row: MemoRow, sameTime: boolean) => {
		// 取り込めない行 (値域違反・整数でない値など) には引き継がない。
		if (row.rowError) return -1;
		const period = dayPeriodAt(row.time, periods);
		if (period === undefined) return -1;
		const systolic = parseStrictInt(row.systolic);
		const diastolic = parseStrictInt(row.diastolic);
		return remaining.findIndex((entry) => {
			const [date, time] = entry.measuredAtLocal.split('T');
			return (
				date === row.measuredOnDate &&
				(sameTime ? time === row.time : dayPeriodAt(time, periods) === period) &&
				entry.systolic === systolic &&
				entry.diastolic === diastolic
			);
		});
	};
	const carried = new Set<MemoRow>();
	for (const sameTime of [true, false]) {
		for (const row of rows) {
			if (carried.has(row) || !canCarry(row)) continue;
			const index = matchIndex(row, sameTime);
			if (index < 0) continue;
			row.memo = remaining[index].memo;
			carried.add(row);
			remaining.splice(index, 1);
		}
	}
}
