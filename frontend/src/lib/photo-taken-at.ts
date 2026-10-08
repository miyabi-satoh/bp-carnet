// 写真の Exif から撮影日時を読む。写真で記録 (血圧計の読み取り) の測定日時の既定値に使う (docs/ocr.md)。
// 昨日撮った写真を後から読み取っても、実際に測った時刻に近い日時が入るように。
//
// 読むのは JPEG の Exif だけ。HEIC などほかの形式や、Exif の無い写真は読めないものとして扱い、
// 呼び出し側は現在時刻を使う。リサイズ (ocr-image.ts) した画像は Exif を持たないので、
// 選んだ元のファイルから読む。

import { isLocalDateTime, toLocalDateTime } from '$lib/period';

/** Exif の入る APP1 は JPEG の先頭近くにあり、最大 64KB。 */
const HEAD_BYTES = 128 * 1024;

const TAG_EXIF_IFD = 0x8769;
const TAG_DATE_TIME_ORIGINAL = 0x9003;
const TAG_DATE_TIME_DIGITIZED = 0x9004;
const TAG_OFFSET_TIME_ORIGINAL = 0x9011;
const TAG_OFFSET_TIME_DIGITIZED = 0x9012;
const TYPE_ASCII = 2;
const TYPE_LONG = 4;

export type ExifDateTime = {
	/** 撮影した端末の時計での日時 (`YYYY-MM-DDTHH:MM`)。 */
	local: string;
	/** UTC からの時差 (`+09:00` など)。Exif に無ければ `null`。 */
	offset: string | null;
};

/** JPEG のバイト列から撮影日時を読む。読めなければ `null`。 */
export function parseExifDateTime(buffer: ArrayBuffer): ExifDateTime | null {
	const view = new DataView(buffer);
	const tiff = findExifTiffStart(view);
	if (tiff === null) return null;
	try {
		const little = readTiffByteOrder(view, tiff);
		if (little === null) return null;
		const ifd0 = tiff + view.getUint32(tiff + 4, little);
		const exifIfdOffset = readIfdEntry(view, tiff, ifd0, little, TAG_EXIF_IFD);
		if (exifIfdOffset === null || exifIfdOffset.type !== TYPE_LONG) return null;
		const exifIfd = tiff + exifIfdOffset.value;

		// 撮った日時 (Original) を優先し、無ければ画像にした日時 (Digitized)。
		for (const [dateTag, offsetTag] of [
			[TAG_DATE_TIME_ORIGINAL, TAG_OFFSET_TIME_ORIGINAL],
			[TAG_DATE_TIME_DIGITIZED, TAG_OFFSET_TIME_DIGITIZED]
		]) {
			const local = toLocal(readAscii(view, tiff, exifIfd, little, dateTag));
			if (local === null) continue;
			const offset = readAscii(view, tiff, exifIfd, little, offsetTag);
			return { local, offset: offset !== null && /^[+-]\d{2}:\d{2}$/.test(offset) ? offset : null };
		}
		return null;
	} catch (e) {
		// 途中で切れた・壊れた Exif は、範囲外を読んだところで RangeError になる。
		if (e instanceof RangeError) return null;
		throw e;
	}
}

/** 写真の撮影日時を、`timeZone` での日時 (`YYYY-MM-DDTHH:MM`) で返す。読めない・今より先なら `null`。
 * 時差が Exif に無ければ、撮影した端末の時計の日時をそのまま `timeZone` での日時とみなす
 * (多くの写真は住んでいる所で撮るため)。 */
export async function photoTakenAt(
	file: Blob,
	timeZone: string,
	now = new Date()
): Promise<string | null> {
	const taken = parseExifDateTime(await file.slice(0, HEAD_BYTES).arrayBuffer());
	if (taken === null) return null;
	if (taken.offset === null) {
		// 端末の時計が狂っている写真は、今より先の日時になることがある。
		return taken.local <= toLocalDateTime(now, timeZone) ? taken.local : null;
	}
	const instant = new Date(`${taken.local}${taken.offset}`);
	if (Number.isNaN(instant.getTime()) || instant > now) return null;
	return toLocalDateTime(instant, timeZone);
}

/** APP1 の Exif の、TIFF ヘッダーの位置。無ければ `null`。 */
function findExifTiffStart(view: DataView): number | null {
	if (view.byteLength < 4 || view.getUint16(0) !== 0xffd8) return null;
	let pos = 2;
	while (pos + 4 <= view.byteLength) {
		if (view.getUint8(pos) !== 0xff) return null;
		const marker = view.getUint8(pos + 1);
		// SOS (画像データ) より後に Exif は来ない。
		if (marker === 0xda || marker === 0xd9) return null;
		const length = view.getUint16(pos + 2);
		if (marker === 0xe1 && pos + 10 <= view.byteLength && isExifHeader(view, pos + 4)) {
			return pos + 10;
		}
		pos += 2 + length;
	}
	return null;
}

/** `Exif\0\0`。 */
function isExifHeader(view: DataView, pos: number): boolean {
	return view.getUint32(pos) === 0x45786966 && view.getUint16(pos + 4) === 0;
}

/** リトルエンディアンなら `true`。TIFF ヘッダーでなければ `null`。 */
function readTiffByteOrder(view: DataView, tiff: number): boolean | null {
	const order = view.getUint16(tiff);
	const little = order === 0x4949 ? true : order === 0x4d4d ? false : null;
	if (little === null || view.getUint16(tiff + 2, little) !== 42) return null;
	return little;
}

type IfdEntry = { type: number; count: number; value: number; valuePos: number };

function readIfdEntry(
	view: DataView,
	tiff: number,
	ifd: number,
	little: boolean,
	tag: number
): IfdEntry | null {
	const count = view.getUint16(ifd, little);
	for (let i = 0; i < count; i++) {
		const entry = ifd + 2 + i * 12;
		if (view.getUint16(entry, little) !== tag) continue;
		const type = view.getUint16(entry + 2, little);
		const n = view.getUint32(entry + 4, little);
		// 4バイトに収まらない値は、TIFF ヘッダーからのオフセットで指される。
		const valuePos =
			type === TYPE_ASCII && n > 4 ? tiff + view.getUint32(entry + 8, little) : entry + 8;
		return { type, count: n, value: view.getUint32(entry + 8, little), valuePos };
	}
	return null;
}

function readAscii(
	view: DataView,
	tiff: number,
	ifd: number,
	little: boolean,
	tag: number
): string | null {
	const entry = readIfdEntry(view, tiff, ifd, little, tag);
	if (entry === null || entry.type !== TYPE_ASCII) return null;
	let text = '';
	for (let i = 0; i < entry.count; i++) {
		const code = view.getUint8(entry.valuePos + i);
		if (code === 0) break;
		text += String.fromCharCode(code);
	}
	return text;
}

/** Exif の `YYYY:MM:DD HH:MM:SS` を `YYYY-MM-DDTHH:MM` にする。形が違う・暦に無い日時 (未設定の
 * `0000:00:00 00:00:00` など) なら `null`。 */
function toLocal(value: string | null): string | null {
	const match = value && /^(\d{4}):(\d{2}):(\d{2}) (\d{2}):(\d{2}):\d{2}$/.exec(value);
	if (!match) return null;
	const [, year, month, day, hour, minute] = match;
	const local = `${year}-${month}-${day}T${hour}:${minute}`;
	return isLocalDateTime(local) ? local : null;
}
