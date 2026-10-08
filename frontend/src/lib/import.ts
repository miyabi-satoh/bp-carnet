import { resolve } from '$app/paths';
import { client } from '$lib/api/client';
import { createFileHandoff } from '$lib/file-handoff';
import { type ApiResult, unwrapRequest } from '$lib/api/errors';
import * as m from '$lib/paraglide/messages.js';
import { type DayPeriod, isValidDateTime, minutesToClockTime, toLocalDateTime } from '$lib/period';
import { firstRecordFormError, splitMeasuredAt, validateRecordForm } from '$lib/record-form';
import {
	fetchRecords,
	type CreateRecordRequest,
	type RecordRef,
	type RecordResponse
} from '$lib/records';
import type { PeriodThresholds } from '$lib/settings';

/** 取り込む1行分。取り込みボタンを押すまでの間、値の編集と採否を保持する。CSV の行も
 * 手書きメモの読み取り結果 (`MemoRow`、`$lib/ocr`) もこの形で扱い、同じ一覧の部品で並べる
 * (docs/import-export.md)。 */
export type ImportRow = {
	/** この行を取り込むか。初期状態はアプリが決める (`initialKeep`)。 */
	keep: boolean;
	/** `<input type="date">` の値 ("YYYY-MM-DD")。 */
	measuredOnDate: string;
	/** `<input type="time">` の値 ("HH:MM")。 */
	time: string;
	/** `time` が、読み取れた値ではなく朝/夜の時間帯から振った仮の値か。一覧では時刻の代わりに
	 * 「朝」「夜」と出す (docs/import-export.md)。 */
	timeIsPlaceholder?: boolean;
	/** 手書きメモの行に書かれていた朝・夜。仮の時刻をこちらの時間帯に振る (`fillPlaceholderTimes`)。 */
	periodHint?: DayPeriod;
	/** 数値の欄は記録フォームと同じく入力されたままの文字列で持つ (`$lib/record-form`)。 */
	systolic: string;
	diastolic: string;
	pulse: string;
	memo: string;
	/** 読み込んだときは「要確認」だったが、シートで直した記録か。一覧で「修正済み」と出す (docs/import-export.md)。 */
	fixed?: boolean;
	/** 値が記録フォームの規則に反する場合の、欄の並び順で最初のエラー。この行は対象 (置き換える
	 * 日の算出・確認画面のdiff・送信) から除外される。値を直して `revalidateRow` を通すことで
	 * 解消できる。 */
	rowError?: string;
};

/** 行の状態 (docs/import-export.md)。赤 (`error`) は直さないと取り込めないときだけ、
 * 橙 (`warn`) は取り込めるが確かめてほしいときに使う。 */
export type ImportRowStatus = 'error' | 'warn' | 'ok';

/** CSV の行の状態。手書きメモの行は読み取りの自信も見る (`$lib/ocr` の `memoRowStatus`)。
 *
 * 仮の時刻を振った行は「注意」にしない: 時刻を書かない使い方が普通で、行ごとに知らせると
 * 全行に並んで邪魔になるため。自動で振ったことは、時刻の代わりに「朝」「夜」と出して伝える
 * (docs/ocr.md)。 */
export function importRowStatus(row: ImportRow): ImportRowStatus {
	return row.rowError ? 'error' : 'ok';
}

/** 行に添える「何をすればよいか」の文 (docs/import-export.md)。CSV の行はエラーのときだけ出す。 */
export function importRowNote(row: ImportRow): string | undefined {
	return row.rowError;
}

/** 読み込んだ直後にチェックを入れるか。そのままでは取り込めない行 (エラー) と、読み取りに
 * 自信のない行 (注意) は外しておく (docs/import-export.md)。 */
export function initialKeep(status: ImportRowStatus): boolean {
	return status === 'ok';
}

/** 取り込むファイルの選択肢を CSV に絞る `<input type="file">` の `accept`。 */
export const CSV_FILE_ACCEPT = '.csv,text/csv';

/** 設定画面で選んだ、取り込みのページへ渡すファイル。 */
export const importFileHandoff = createFileHandoff(resolve('/settings/import'));

// --- CSV パース -------------------------------------------------------------

/** RFC 4180 相当の CSV パーサー (状態機械)。`""` エスケープ、クォート内の `,`/改行、
 * `\r\n`/`\n` 混在に対応する。ライブラリを使わず自前実装するのは、エクスポート
 * (`GET /records/export`) が生成する固定フォーマットの読み戻し専用の用途であり、
 * 型定義を持つ軽量な既存ライブラリが無いため。空行 (フィールドが1個だけで空文字列)
 * は読み飛ばす。 */
function parseCsvTable(text: string): string[][] {
	const rows: string[][] = [];
	let row: string[] = [];
	let field = '';
	let inQuotes = false;
	let i = 0;
	const len = text.length;

	const pushField = () => {
		row.push(field);
		field = '';
	};
	const pushRow = () => {
		pushField();
		rows.push(row);
		row = [];
	};

	while (i < len) {
		const c = text[i];
		if (inQuotes) {
			if (c === '"') {
				if (text[i + 1] === '"') {
					field += '"';
					i += 2;
				} else {
					inQuotes = false;
					i += 1;
				}
			} else {
				field += c;
				i += 1;
			}
			continue;
		}
		if (c === '"') {
			inQuotes = true;
			i += 1;
		} else if (c === ',') {
			pushField();
			i += 1;
		} else if (c === '\r') {
			pushRow();
			i += 1;
			if (text[i] === '\n') i += 1;
		} else if (c === '\n') {
			pushRow();
			i += 1;
		} else {
			field += c;
			i += 1;
		}
	}
	if (field !== '' || row.length > 0) pushRow();

	return rows.filter((r) => !(r.length === 1 && r[0] === ''));
}

/** CSV ファイルのバイト列を文字列にする。UTF-8 として読めなければ Shift_JIS として読む。日本語の Excel で
 * 「CSV (コンマ区切り)」を選んで保存すると Shift_JIS になるため (docs/import-export.md)。 */
export function decodeCsvText(bytes: ArrayBuffer): string {
	try {
		return new TextDecoder('utf-8', { fatal: true }).decode(bytes);
	} catch {
		return new TextDecoder('shift_jis').decode(bytes);
	}
}

// --- 数値・日時パース ---------------------------------------------------------

/** CSV の測定日時を、個人設定のタイムゾーンでの日付・時刻の2欄にする。
 * - オフセット付き (UTC 等) はそのタイムゾーンに換算し、オフセット無しはそのタイムゾーンの時刻とみなす。
 * - オフセット無しは `2026-09-01 07:10` のほか、日本語の Excel が保存し直した `2026/9/1 7:10`
 *   (区切りが `/`、月・日・時がゼロ埋めでない) も読む (docs/import-export.md)。
 * - 日付だけ (`2026-09-01`・`2026/9/1`) なら時刻を空で返す。仮の時刻は後で振る (`fillPlaceholderTimes`、docs/import-export.md)。
 * - 解釈できなければ `null`。記録は分単位で扱うため、秒が 0 でない日時も `null` にする (切り捨てると、
 *   取り込みで記録の日時が黙って変わるため)。 */
function csvMeasuredAt(
	raw: string,
	timeZone: string
): { measuredOnDate: string; time: string } | null {
	const trimmed = raw.trim();
	const pad = (value: string) => value.padStart(2, '0');
	const local =
		/^(\d{4})([-/])(\d{1,2})\2(\d{1,2})(?:[T ](\d{1,2}):(\d{2})(?::(\d{2})(?:\.(\d+))?)?)?$/.exec(
			trimmed
		);
	if (local) {
		const [, yearText, , monthText, dayText, hourText, minuteText, seconds, fraction] = local;
		const measuredOnDate = `${yearText}-${pad(monthText)}-${pad(dayText)}`;
		const [year, month, day] = [yearText, monthText, dayText].map(Number);
		if (hourText === undefined) {
			return isValidDateTime(year, month, day) ? { measuredOnDate, time: '' } : null;
		}
		if (!isValidDateTime(year, month, day, Number(hourText), Number(minuteText))) return null;
		if (
			(seconds !== undefined && Number(seconds) !== 0) ||
			(fraction !== undefined && !/^0+$/.test(fraction))
		) {
			return null;
		}
		return { measuredOnDate, time: `${pad(hourText)}:${minuteText}` };
	}
	if (!/(Z|[+-]\d{2}:?\d{2})$/i.test(trimmed)) return null;
	const date = new Date(trimmed);
	if (
		Number.isNaN(date.getTime()) ||
		date.getUTCSeconds() !== 0 ||
		date.getUTCMilliseconds() !== 0
	) {
		return null;
	}
	return splitMeasuredAt(toLocalDateTime(date, timeZone));
}

// --- 仮の時刻 -----------------------------------------------------------------

/** 時間帯 `period` の中で `offset` 番目 (0始まり) に振る仮時刻 (HH:MM)。時間帯の中央から1分ずつずらし、
 * 時間帯の終了時刻以降に出ないよう丸める。 */
function periodTime(period: DayPeriod, offset: number, periods: PeriodThresholds): string {
	const [start, end] =
		period === 'morning'
			? [periods.morningStartMin, periods.morningEndMin]
			: [periods.eveningStartMin, periods.eveningEndMin];
	// `<input type="time">` と記録フォームの検証は、時もゼロ埋めした HH:MM を前提にする。
	return minutesToClockTime(Math.min(Math.floor((start + end) / 2) + offset, end - 1));
}

/** `timeIsPlaceholder` の行 (時刻の無い行) に仮時刻を振る。手書きメモの読み取りと CSV で同じ規則にする
 * (docs/import-export.md)。
 * - 日付ごとに時刻の無い行を数え、前半を朝・後半を夜の枠にする (奇数件なら朝を1件多く)。
 * - 朝・夜が書かれていた行 (`periodHint`) は、その時間帯に振る。書かれていない行は、書かれた行が使った残りの
 *   枠に並び順で入る (「夜」と書いた行のある日の、書かれていない行を朝にする)。
 * - 同じ日・同じ時間帯の行は、並び順を保つよう1分ずつずらす。
 * 日付が無い行には振らない (日時の欄のエラーとして直してもらうため)。 */
export function fillPlaceholderTimes(rows: ImportRow[], periods: PeriodThresholds): void {
	const byDate = new Map<string, ImportRow[]>();
	for (const row of rows) {
		if (!row.timeIsPlaceholder || row.measuredOnDate === '') continue;
		byDate.set(row.measuredOnDate, [...(byDate.get(row.measuredOnDate) ?? []), row]);
	}
	for (const untimed of byDate.values()) {
		const hinted = (period: DayPeriod) => untimed.filter((row) => row.periodHint === period).length;
		let morningLeft = Math.ceil(untimed.length / 2) - hinted('morning');
		const used: Record<DayPeriod, number> = { morning: 0, evening: 0 };
		for (const row of untimed) {
			let period = row.periodHint;
			if (!period) {
				period = morningLeft > 0 ? 'morning' : 'evening';
				if (period === 'morning') morningLeft--;
			}
			row.time = periodTime(period, used[period]++, periods);
		}
	}
}

// --- 検証 -------------------------------------------------------------------

/** 記録フォームと同じ規則で検証し直し、`rowError` を更新する。 */
export function revalidateRow(row: ImportRow): void {
	row.rowError = firstRecordFormError(row);
}

/** 読み込んだ直後の行を検証し (`revalidateRow`)、チェックの初期状態を決める (`initialKeep`)。
 * 仮時刻を振った後に呼ぶ (時刻の無い行を「入力してください」にしないため)。 */
export function settleNewRows<T extends ImportRow>(
	rows: T[],
	statusOf: (row: T) => ImportRowStatus
): void {
	for (const row of rows) {
		revalidateRow(row);
		row.keep = initialKeep(statusOf(row));
	}
}

/** シートで直した値を行に書き戻す (`record-form-sheet.svelte` の `onsubmit`)。値はシートが
 * 検証済み。CSV でも手書きメモでも同じ扱いにするため、ここ1箇所にまとめる。
 *
 * チェックを入れるのは、シートを開いて「更新する」まで押したのは取り込みたいからで、
 * 確かめたのに取り込まれないのは分かりにくいため。読み込んだ直後にアプリが外した行
 * (エラー・自信のない行) を、本人の確認で戻す道でもある。 */
export function applyEditedRow(row: ImportRow, record: CreateRecordRequest): void {
	Object.assign(row, splitMeasuredAt(record.localMeasuredAt), {
		systolic: String(record.systolic),
		diastolic: String(record.diastolic),
		pulse: record.pulse === undefined || record.pulse === null ? '' : String(record.pulse),
		memo: record.memo ?? '',
		// 直した時刻は本人が入れた値なので、仮の時刻ではなくなる。
		timeIsPlaceholder: false,
		keep: true
	});
	revalidateRow(row);
}

// --- 行の組み立て ------------------------------------------------------------

/** 5フィールド (文字列) から `ImportRow` を組み立てる。検証とチェックの初期状態は、仮時刻を
 * 振った後に `parseCsvImport` が決める。 */
function rowFromFields(
	timeZone: string,
	measuredAtRaw: string,
	systolicRaw: string,
	diastolicRaw: string,
	pulseRaw: string,
	memoRaw: string
): ImportRow {
	const measuredAt = csvMeasuredAt(measuredAtRaw, timeZone);
	return {
		keep: true,
		// 解釈できない日時は空にする。日時の欄も空で出るので、記録フォームと同じ「入力してください」になる。
		measuredOnDate: measuredAt?.measuredOnDate ?? '',
		time: measuredAt?.time ?? '',
		timeIsPlaceholder: measuredAt !== null && measuredAt.time === '',
		systolic: systolicRaw.trim(),
		diastolic: diastolicRaw.trim(),
		pulse: pulseRaw.trim(),
		memo: memoRaw
	};
}

const REQUIRED_CSV_HEADERS = ['measuredAt', 'systolic', 'diastolic'] as const;

export type ParseResult = { rows: ImportRow[] } | { error: string };

/** CSV (エクスポートが生成した形式、または同じ列名を持つもの) をパースする。列は名前で
 * マッピングする (余分な列は無視、順序も問わない)。先頭の UTF-8
 * BOM は取り除く (エクスポートが Excel 対策で付与しているため)。`timeZone` は個人設定のタイムゾーン、
 * `periods` は時刻の無い行に仮時刻を振るための朝/夜の時間帯。 */
export function parseCsvImport(
	text: string,
	timeZone: string,
	periods: PeriodThresholds
): ParseResult {
	const stripped = text.replace(/^\uFEFF/, '');
	const table = parseCsvTable(stripped);
	if (table.length === 0) {
		return { error: m.import_error_csv_no_header() };
	}

	const header = table[0].map((h) => h.trim());
	const missing = REQUIRED_CSV_HEADERS.filter((h) => !header.includes(h));
	if (missing.length > 0) {
		return { error: m.import_error_csv_missing_columns({ columns: missing.join(', ') }) };
	}

	const indexOf = (name: string) => header.indexOf(name);
	const idx = {
		measuredAt: indexOf('measuredAt'),
		systolic: indexOf('systolic'),
		diastolic: indexOf('diastolic'),
		pulse: indexOf('pulse'),
		memo: indexOf('memo')
	};

	const rows = table
		.slice(1)
		.map((cols) =>
			rowFromFields(
				timeZone,
				cols[idx.measuredAt] ?? '',
				cols[idx.systolic] ?? '',
				cols[idx.diastolic] ?? '',
				idx.pulse >= 0 ? (cols[idx.pulse] ?? '') : '',
				idx.memo >= 0 ? (cols[idx.memo] ?? '') : ''
			)
		);
	fillPlaceholderTimes(rows, periods);
	settleNewRows(rows, importRowStatus);
	return { rows };
}

// --- 対象の日付・範囲の算出 --------------------------------------------------

/** チェックが入っていて、値にエラーの無い行 (取り込みの対象)。 */
function includedRows(rows: ImportRow[]): ImportRow[] {
	return rows.filter((row) => row.keep && !row.rowError);
}

/** 対象行の最小日付〜最大日付 (ローカル日、`YYYY-MM-DD`)。対象行が1件も無ければ `null`
 * (取り込むものが無く、実行できない)。
 *
 * これは置き換える範囲ではない。置き換えるのは対象行がある日だけで (docs/import-export.md)、この範囲は
 * 既存レコードの取得と、送った行がその中に収まっているかのサーバー側の検証に使う。 */
export function importDateRange(rows: ImportRow[]): { from: string; to: string } | null {
	let from: string | null = null;
	let to: string | null = null;
	for (const row of includedRows(rows)) {
		const date = row.measuredOnDate;
		if (from === null || date < from) from = date;
		if (to === null || date > to) to = date;
	}
	return from === null || to === null ? null : { from, to };
}

// --- 確認画面のdiff -----------------------------------------------------------

/** 行の内容を比較するための共通の形。CSV由来 (`ImportRow`)・既存レコード
 * (`RecordResponse`) の両方をこの形に揃えてから突き合わせる。 */
export type ImportEntry = {
	measuredAtLocal: string;
	systolic: number;
	diastolic: number;
	pulse: number | undefined;
	memo: string;
};

function entryFromRow(row: ImportRow): ImportEntry {
	// includedRows を通過した行は revalidateRow/rowFromFields で検証済みのはず。前提が崩れた場合
	// (将来の変更で未検証の行が紛れ込む等) に、壊れたdiffや無効な値を静かに送るのではなく
	// ここで気付けるようにする。
	const result = validateRecordForm(row);
	if (!result.ok) {
		throw new Error('entryFromRow: an included row must pass validation');
	}
	return {
		measuredAtLocal: result.measuredAtLocal,
		systolic: result.systolic,
		diastolic: result.diastolic,
		pulse: result.pulse,
		memo: row.memo
	};
}

/** 既存の記録の `ImportEntry`。取り込むときに、確認画面に出した時点の版番号として送る (docs/import-export.md)。 */
export type ExistingEntry = ImportEntry & RecordRef;

function entryFromRecord(record: RecordResponse): ExistingEntry {
	return {
		id: record.id,
		version: record.version,
		measuredAtLocal: record.localMeasuredAt,
		systolic: record.systolic,
		diastolic: record.diastolic,
		pulse: record.pulse ?? undefined,
		memo: record.memo
	};
}

/** 突き合わせ用のキー。memo (自由記述) を最後尾に置くため、memo に `|` が含まれても
 * 他フィールドとの境界は曖昧にならない。 */
function diffKey(entry: ImportEntry): string {
	return `${entry.measuredAtLocal}|${entry.systolic}|${entry.diastolic}|${entry.pulse ?? ''}|${entry.memo}`;
}

export type ImportDiffStatus = 'added' | 'removed' | 'unchanged';

export type ImportDiffRow = ImportEntry & { status: ImportDiffStatus };

export type ImportDiffGroup = {
	/** ローカル日 (`YYYY-MM-DD`)。 */
	date: string;
	/** この日付に `removed` な行が1件でもあるか (確認画面で目立たせる対象)。 */
	hasRemoval: boolean;
	/** 測定時刻の昇順。同じ時刻は `removed` → `unchanged` → `added` (元の記録が消えて新しい記録が入る流れ)。 */
	rows: ImportDiffRow[];
};

const STATUS_ORDER: Record<ImportDiffStatus, number> = { removed: 0, unchanged: 1, added: 2 };

/** 対象行 (CSV) と既存レコードを突き合わせ、日付ごとの `追加`/`削除`/`変更なし` に分類する
 * (docs/import-export.md)。完全一致 (measuredAt・systolic・diastolic・pulse・memo) を「変更なし」
 * とする多重集合マッチ: 同じ内容の行が複数あっても、件数分だけ対応づける。
 *
 * 置き換えるのは対象行がある日だけなので、対象行が1件も無い日の既存レコードは、範囲の中に
 * あっても触らない (`removed` にしない・その日のグループを作らない)。 */
export function buildImportDiff(
	rows: ImportRow[],
	existingEntries: ImportEntry[]
): ImportDiffGroup[] {
	const included = includedRows(rows);
	const targetDates = new Set(included.map((row) => row.measuredOnDate));

	const remainingExisting = new Map<string, ImportEntry[]>();
	for (const entry of existingEntries) {
		if (!targetDates.has(entry.measuredAtLocal.slice(0, 10))) continue;
		const key = diffKey(entry);
		const bucket = remainingExisting.get(key);
		if (bucket) bucket.push(entry);
		else remainingExisting.set(key, [entry]);
	}

	const diffRows: ImportDiffRow[] = [];
	for (const row of included) {
		const entry = entryFromRow(row);
		const key = diffKey(entry);
		const bucket = remainingExisting.get(key);
		if (bucket && bucket.length > 0) {
			bucket.pop();
			diffRows.push({ ...entry, status: 'unchanged' });
		} else {
			diffRows.push({ ...entry, status: 'added' });
		}
	}
	for (const bucket of remainingExisting.values()) {
		for (const entry of bucket) diffRows.push({ ...entry, status: 'removed' });
	}

	const groups = new Map<string, ImportDiffRow[]>();
	for (const row of diffRows) {
		const date = row.measuredAtLocal.slice(0, 10);
		const bucket = groups.get(date);
		if (bucket) bucket.push(row);
		else groups.set(date, [row]);
	}

	return [...groups.entries()]
		.sort(([a], [b]) => a.localeCompare(b))
		.map(([date, groupRows]) => ({
			date,
			hasRemoval: groupRows.some((row) => row.status === 'removed'),
			rows: groupRows.sort(
				(a, b) =>
					a.measuredAtLocal.localeCompare(b.measuredAtLocal) ||
					STATUS_ORDER[a.status] - STATUS_ORDER[b.status]
			)
		}));
}

/** 削除される記録があれば、日付や件数を挙げない警告文。無ければ `undefined`。 */
export function deletionWarning(groups: ImportDiffGroup[]): string | undefined {
	return groups.some((group) => group.hasRemoval) ? m.import_preview_deletion_warning() : undefined;
}

/** 対象範囲 (ローカル日) の既存レコードを取得し、diff用の `ImportEntry` に変換する。 */
export async function fetchExistingEntries(range: {
	from: string;
	to: string;
}): Promise<ApiResult<{ entries: ExistingEntry[] }>> {
	const result = await fetchRecords(range);
	if (!result.ok) return result;
	return { ok: true, entries: result.data.map(entryFromRecord) };
}

// --- インポート実行 ----------------------------------------------------------

/** テストのため export する。 */
export function rowToRequest(row: ImportRow): CreateRecordRequest {
	const { measuredAtLocal, ...values } = entryFromRow(row);
	return { localMeasuredAt: measuredAtLocal, ...values };
}

export type ImportOutcome = ApiResult<{ deleted: number; created: number }>;

/** 対象行がある日を1トランザクションで置き換える (`POST /records/import`)。行が1件も無い日の
 * 記録には触らない。バックエンド側も全行検証してから削除・挿入するため、失敗時は何も変わらない
 * (中間状態が残らない)。
 *
 * `existing` は確認画面に出した既存の記録。置き換える日の分を送り、確認画面を出してから
 * ほかで変わっていれば `record_conflict` で止めてもらう (docs/import-export.md)。 */
export async function importRows(
	rows: ImportRow[],
	existing: ExistingEntry[]
): Promise<ImportOutcome> {
	const range = importDateRange(rows);
	if (range === null) {
		throw new Error('importRows: no rows to import');
	}
	const included = includedRows(rows);
	const targetDates = new Set(included.map((row) => row.measuredOnDate));

	const result = await unwrapRequest(
		client.POST('/api/v1/records/import', {
			body: {
				from: range.from,
				to: range.to,
				records: included.map(rowToRequest),
				expected: existing
					.filter((entry) => targetDates.has(entry.measuredAtLocal.slice(0, 10)))
					.map(({ id, version }) => ({ id, version }))
			}
		})
	);
	if (!result.ok) return result;
	return { ok: true, deleted: result.data.deleted, created: result.data.created.length };
}
