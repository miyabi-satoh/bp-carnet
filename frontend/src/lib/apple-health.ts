import { importRowStatus, settleNewRows, type ImportRow } from '$lib/import';
import { instantWithOffset, toLocalDateTime } from '$lib/period';
import { splitMeasuredAt } from '$lib/record-form';

/** Apple の「ヘルスケア」が書き出したデータ (`書き出したデータ.zip`、中の `export.xml`) から、血圧の記録を
 * 取り出す (docs/import-export.md「他のアプリのファイル」)。形が決まっているので Gemini は使わず、
 * ブラウザの中だけで読む。ファイルは 1GB を超えることがあるので、全体をメモリに載せず、流しながら探す。 */

/** 読めなかった理由。`unsupported` はブラウザが zip の展開 (`DecompressionStream`) に対応していない、
 * `not_export` は「ヘルスケア」が書き出したファイルでない。 */
export class AppleHealthError extends Error {
	constructor(readonly reason: 'unsupported' | 'not_export') {
		super(`cannot read the Apple Health export: ${reason}`);
	}
}

/** 血圧の `<Record>` の `type` に共通する部分 (`…BloodPressureSystolic`・`…BloodPressureDiastolic`)。 */
const BP_TYPE = 'HKQuantityTypeIdentifierBloodPressure';
const SYSTOLIC_TYPE = `${BP_TYPE}Systolic`;
const DIASTOLIC_TYPE = `${BP_TYPE}Diastolic`;
/** 心拍数の `<Record>` の `type` の属性。閉じる `"` まで含め、`…HeartRateVariabilitySDNN` などを除く。 */
const HEART_RATE_ATTR = 'type="HKQuantityTypeIdentifierHeartRate"';

/** 1 kPa あたりの mmHg。「ヘルスケア」の単位を kPa にしている端末の値を mmHg に直す。 */
const MMHG_PER_KPA = 7.50062;

// --- zip -------------------------------------------------------------------

const EOCD_SIGNATURE = 0x06054b50;
const ZIP64_LOCATOR_SIGNATURE = 0x07064b50;
const ZIP64_EOCD_SIGNATURE = 0x06064b50;
const CENTRAL_HEADER_SIGNATURE = 0x02014b50;
const LOCAL_HEADER_SIGNATURE = 0x04034b50;
/** EOCD の固定部分 (22 バイト) と、その後ろに付きうるコメントの最大 (65,535 バイト)、ZIP64 の位置の記録 (20 バイト)。 */
const EOCD_SEARCH_BYTES = 22 + 0xffff + 20;
const ZIP64_EXTRA_ID = 0x0001;

type ZipEntry = { name: string; method: number; compressedSize: number; localHeaderOffset: number };

async function viewOf(file: Blob, start: number, end: number): Promise<DataView> {
	return new DataView(await file.slice(start, end).arrayBuffer());
}

/** zip の中のファイルの一覧 (セントラルディレクトリ)。zip でなければ `null`。 */
async function zipEntries(file: File): Promise<ZipEntry[] | null> {
	const tailStart = Math.max(0, file.size - EOCD_SEARCH_BYTES);
	const tail = await viewOf(file, tailStart, file.size);
	let eocd = -1;
	for (let i = tail.byteLength - 22; i >= 0; i--) {
		if (tail.getUint32(i, true) === EOCD_SIGNATURE) {
			eocd = i;
			break;
		}
	}
	if (eocd < 0) return null;

	let directorySize = tail.getUint32(eocd + 12, true);
	let directoryOffset = tail.getUint32(eocd + 16, true);
	// 4GB を超える zip (ZIP64) は、本当の値を別の記録に持つ。
	if (directoryOffset === 0xffffffff || directorySize === 0xffffffff) {
		const locator = eocd - 20;
		if (locator < 0 || tail.getUint32(locator, true) !== ZIP64_LOCATOR_SIGNATURE) return null;
		const zip64Offset = Number(tail.getBigUint64(locator + 8, true));
		const zip64 = await viewOf(file, zip64Offset, zip64Offset + 56);
		if (zip64.byteLength < 56 || zip64.getUint32(0, true) !== ZIP64_EOCD_SIGNATURE) return null;
		directorySize = Number(zip64.getBigUint64(40, true));
		directoryOffset = Number(zip64.getBigUint64(48, true));
	}

	const directory = await viewOf(file, directoryOffset, directoryOffset + directorySize);
	const decoder = new TextDecoder();
	const entries: ZipEntry[] = [];
	let pos = 0;
	while (
		pos + 46 <= directory.byteLength &&
		directory.getUint32(pos, true) === CENTRAL_HEADER_SIGNATURE
	) {
		const method = directory.getUint16(pos + 10, true);
		let compressedSize = directory.getUint32(pos + 20, true);
		const uncompressedSize = directory.getUint32(pos + 24, true);
		const nameLength = directory.getUint16(pos + 28, true);
		const extraLength = directory.getUint16(pos + 30, true);
		const commentLength = directory.getUint16(pos + 32, true);
		let localHeaderOffset = directory.getUint32(pos + 42, true);
		const nameStart = pos + 46;
		const name = decoder.decode(
			new Uint8Array(directory.buffer, directory.byteOffset + nameStart, nameLength)
		);

		// ZIP64 の追加の欄には、32 ビットに収まらなかった値だけが、展開後の大きさ・圧縮後の大きさ・位置の順に並ぶ。
		let extra = nameStart + nameLength;
		const extraEnd = extra + extraLength;
		while (extra + 4 <= extraEnd) {
			const id = directory.getUint16(extra, true);
			const size = directory.getUint16(extra + 2, true);
			if (id === ZIP64_EXTRA_ID) {
				let field = extra + 4;
				if (uncompressedSize === 0xffffffff) field += 8;
				if (compressedSize === 0xffffffff) {
					compressedSize = Number(directory.getBigUint64(field, true));
					field += 8;
				}
				if (localHeaderOffset === 0xffffffff) {
					localHeaderOffset = Number(directory.getBigUint64(field, true));
				}
			}
			extra += 4 + size;
		}

		entries.push({ name, method, compressedSize, localHeaderOffset });
		pos = extraEnd + commentLength;
	}
	return entries;
}

/** zip の中の、記録の本体の XML。iOS の表示言語で名前が変わる (`export.xml`・`書き出したデータ.xml`) ので
 * 名前では決めず、XML のうち診療の文書 (`export_cda.xml`) でないものを選ぶ。 */
function exportEntry(entries: ZipEntry[]): ZipEntry | undefined {
	return entries.find((entry) => {
		const base = entry.name.split('/').pop() ?? '';
		return /\.xml$/i.test(base) && !/cda/i.test(base);
	});
}

type ByteSource = {
	stream: () => ReadableStream<Uint8Array>;
	size: number;
	/** 流れが deflate で圧縮されているか (zip の中のファイル)。 */
	deflated: boolean;
};

/** 選んだファイルから、`export.xml` の中身を流す元を作る。2回読む (血圧・心拍数) ので、流れそのものでなく
 * 作り方を返す。`size` は、進み具合を出すための、読み進める元 (zip なら圧縮されたまま) の大きさ。 */
async function exportXmlSource(file: File): Promise<ByteSource> {
	try {
		return await locateExportXml(file);
	} catch (error) {
		// 壊れた zip は、長さや位置の欄がファイルの外を指し、読み取りが RangeError になる。
		if (error instanceof RangeError) throw new AppleHealthError('not_export');
		throw error;
	}
}

async function locateExportXml(file: File): Promise<ByteSource> {
	const entries = await zipEntries(file);
	if (entries === null) {
		// 名前が zip なのに zip として読めないファイルは、壊れているか別のもの。
		if (/\.zip$/i.test(file.name)) throw new AppleHealthError('not_export');
		// zip でなければ、取り出し済みの export.xml として読む。
		return { stream: () => file.stream(), size: file.size, deflated: false };
	}

	const entry = exportEntry(entries);
	if (!entry) throw new AppleHealthError('not_export');
	const header = await viewOf(file, entry.localHeaderOffset, entry.localHeaderOffset + 30);
	if (header.byteLength < 30 || header.getUint32(0, true) !== LOCAL_HEADER_SIGNATURE) {
		throw new AppleHealthError('not_export');
	}
	const dataStart =
		entry.localHeaderOffset + 30 + header.getUint16(26, true) + header.getUint16(28, true);
	const data = file.slice(dataStart, dataStart + entry.compressedSize);
	if (entry.method === 0) return { stream: () => data.stream(), size: data.size, deflated: false };
	// 8 は deflate。zip はほぼこれだけを使う。
	if (entry.method !== 8) throw new AppleHealthError('not_export');
	if (typeof DecompressionStream === 'undefined') throw new AppleHealthError('unsupported');
	return { stream: () => data.stream(), size: data.size, deflated: true };
}

// --- XML を流しながら探す -----------------------------------------------------

/** 閉じていないタグを持ち越す長さの上限 (文字数)。記録の `<Record … />` は数百文字なので、これを超えるのは
 * 壊れたファイル。上限が無いと、`>` の無いファイルで全体をメモリにためてしまう。 */
const MAX_OPEN_TAG_CHARS = 1024 * 1024;

/** `source` を流し、`needle` を含むタグ (`<…>`) ごとに `ontag` を呼ぶ。XML 全体は解釈しない: 記録は
 * 1件が1つのタグ (`<Record … />`) で、探すのはその属性だけなので、文字列の検索で足りる。 */
async function scanTags(
	source: ByteSource,
	needle: string,
	ontag: (tag: string) => void,
	onprogress: (fraction: number) => void
): Promise<void> {
	let read = 0;
	const counted = source.stream().pipeThrough(
		new TransformStream<Uint8Array, Uint8Array>({
			transform(chunk, controller) {
				read += chunk.byteLength;
				controller.enqueue(chunk);
			}
		})
	);
	const bytes = source.deflated
		? counted.pipeThrough(
				// FIX: lib.dom の型で、`DecompressionStream` の書き込み側が `BufferSource` になっており、
				// `Uint8Array` の流れをそのままつなげない。
				new DecompressionStream('deflate-raw') as unknown as TransformStream<Uint8Array, Uint8Array>
			)
		: counted;
	const reader = bytes.getReader();
	const decoder = new TextDecoder();
	/** 前の塊の末尾の、閉じていないタグ。次の塊の頭につなげる。 */
	let carry = '';
	for (;;) {
		const { done, value } = await reader.read();
		const text = carry + (done ? decoder.decode() : decoder.decode(value, { stream: true }));
		let processed = 0;
		for (let hit = text.indexOf(needle); hit >= 0; hit = text.indexOf(needle, processed)) {
			const start = text.lastIndexOf('<', hit);
			const end = text.indexOf('>', hit);
			if (end < 0) break;
			if (start >= processed) ontag(text.slice(start, end + 1));
			processed = end + 1;
		}
		// 閉じていない最後のタグは、続きが次の塊にある。
		const lastOpen = text.lastIndexOf('<');
		carry = lastOpen >= processed && text.indexOf('>', lastOpen) < 0 ? text.slice(lastOpen) : '';
		if (carry.length > MAX_OPEN_TAG_CHARS) {
			await reader.cancel();
			throw new AppleHealthError('not_export');
		}
		onprogress(source.size === 0 ? 1 : Math.min(1, read / source.size));
		if (done) return;
	}
}

function attribute(tag: string, name: string): string | undefined {
	const start = tag.indexOf(` ${name}="`);
	if (start < 0) return undefined;
	const valueStart = start + name.length + 3;
	const end = tag.indexOf('"', valueStart);
	return end < 0 ? undefined : tag.slice(valueStart, end);
}

// --- 記録の組み立て -----------------------------------------------------------

type Pressure = { systolic?: number; diastolic?: number };

/** 「ヘルスケア」の書き出しから、血圧の記録を取り込みの行にする。`onprogress` には 0〜1 の進み具合を渡す。
 *
 * - 収縮期と拡張期は別々の `<Record>` で、同じ測定は `startDate` が同じ。`<Correlation>` の中にも同じ
 *   `<Record>` が重ねて書かれるので、`startDate` で1件にまとめる。
 * - 脈拍は、血圧と同じ `startDate` の心拍数の記録があれば使う (血圧計のアプリが測定と一緒に書くもの)。
 *   心拍数は血圧より後ろに書かれているとは限らないので、血圧の日時を集めてからもう一度流して探す。
 * - 秒は捨てる (記録は分単位)。 */
export async function readAppleHealthExport(
	file: File,
	timeZone: string,
	onprogress: (fraction: number) => void = () => {}
): Promise<ImportRow[]> {
	const source = await exportXmlSource(file);

	const pressures = new Map<string, Pressure>();
	await scanTags(
		source,
		BP_TYPE,
		(tag) => {
			if (!tag.startsWith('<Record ')) return;
			const type = attribute(tag, 'type');
			const startDate = attribute(tag, 'startDate');
			const rawValue = attribute(tag, 'value');
			// 値の無い記録を 0 として読まない (`Number('')` は 0)。
			if (startDate === undefined || !rawValue) return;
			const value = Number(rawValue);
			if (!Number.isFinite(value)) return;
			const mmHg = Math.round(attribute(tag, 'unit') === 'kPa' ? value * MMHG_PER_KPA : value);
			const pressure = pressures.get(startDate) ?? {};
			if (type === SYSTOLIC_TYPE) pressure.systolic = mmHg;
			else if (type === DIASTOLIC_TYPE) pressure.diastolic = mmHg;
			else return;
			pressures.set(startDate, pressure);
		},
		(fraction) => onprogress(fraction / 2)
	);
	if (pressures.size === 0) {
		onprogress(1);
		return [];
	}

	const pulses = new Map<string, number>();
	await scanTags(
		source,
		HEART_RATE_ATTR,
		(tag) => {
			const startDate = attribute(tag, 'startDate');
			if (startDate === undefined || !pressures.has(startDate)) return;
			const value = Number(attribute(tag, 'value'));
			if (Number.isFinite(value)) pulses.set(startDate, Math.round(value));
		},
		(fraction) => onprogress(0.5 + fraction / 2)
	);

	const rows: { at: number; row: ImportRow }[] = [];
	for (const [startDate, { systolic, diastolic }] of pressures) {
		// 片方だけの記録 (手で片方だけ入れたもの) は、血圧の記録にならない。
		if (systolic === undefined || diastolic === undefined) continue;
		const instant = instantWithOffset(startDate);
		if (!instant) continue;
		rows.push({
			at: instant.getTime(),
			row: {
				keep: true,
				...splitMeasuredAt(toLocalDateTime(instant, timeZone)),
				systolic: String(systolic),
				diastolic: String(diastolic),
				pulse: String(pulses.get(startDate) ?? ''),
				memo: ''
			}
		});
	}
	const result = rows.sort((a, b) => a.at - b.at).map(({ row }) => row);
	settleNewRows(result, importRowStatus);
	return result;
}
