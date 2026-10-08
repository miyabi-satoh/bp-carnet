// テスト用: JPEG に撮影日時の Exif を足す (photo-taken-at.spec.ts と e2e で共通)。

export type ExifFields = {
	/** Exif の書き方 (`YYYY:MM:DD HH:MM:SS`)。 */
	dateTimeOriginal?: string;
	/** `+09:00` など。 */
	offsetTimeOriginal?: string;
	dateTimeDigitized?: string;
	/** 既定はビッグエンディアン (`MM`)。 */
	littleEndian?: boolean;
};

const TYPE_ASCII = 2;
const TYPE_LONG = 4;

/** `jpeg` の SOI の直後に、`fields` を持つ Exif の APP1 を差し込んだバイト列を返す。 */
export function withExif(jpeg: Uint8Array, fields: ExifFields): Uint8Array<ArrayBuffer> {
	const tiff = buildTiff(fields);
	const app1Length = 2 + 6 + tiff.length;
	const app1 = new Uint8Array(2 + app1Length);
	const view = new DataView(app1.buffer);
	view.setUint16(0, 0xffe1);
	view.setUint16(2, app1Length);
	app1.set([0x45, 0x78, 0x69, 0x66, 0, 0], 4);
	app1.set(tiff, 10);

	const out = new Uint8Array(jpeg.length + app1.length);
	out.set(jpeg.subarray(0, 2), 0);
	out.set(app1, 2);
	out.set(jpeg.subarray(2), 2 + app1.length);
	return out;
}

function buildTiff(fields: ExifFields): Uint8Array<ArrayBuffer> {
	const little = fields.littleEndian ?? false;
	const tags: [number, string | undefined][] = [
		[0x9003, fields.dateTimeOriginal],
		[0x9004, fields.dateTimeDigitized],
		[0x9011, fields.offsetTimeOriginal]
	];
	const ascii = tags.filter((entry): entry is [number, string] => entry[1] !== undefined);

	const ifd0 = 8;
	const ifd0Size = 2 + 12 + 4;
	const exifIfd = ifd0 + ifd0Size;
	const exifIfdSize = 2 + ascii.length * 12 + 4;
	let dataPos = exifIfd + exifIfdSize;
	const strings = ascii.map(([, value]) => new TextEncoder().encode(`${value}\0`));
	const total = dataPos + strings.reduce((sum, s) => sum + s.length, 0);

	const bytes = new Uint8Array(total);
	const view = new DataView(bytes.buffer);
	view.setUint16(0, little ? 0x4949 : 0x4d4d);
	view.setUint16(2, 42, little);
	view.setUint32(4, ifd0, little);

	view.setUint16(ifd0, 1, little);
	writeEntry(view, ifd0 + 2, little, 0x8769, TYPE_LONG, 1, exifIfd);
	view.setUint32(ifd0 + 14, 0, little);

	view.setUint16(exifIfd, ascii.length, little);
	ascii.forEach(([tag], i) => {
		const s = strings[i];
		const entry = exifIfd + 2 + i * 12;
		if (s.length <= 4) {
			writeEntry(view, entry, little, tag, TYPE_ASCII, s.length, 0);
			bytes.set(s, entry + 8);
		} else {
			writeEntry(view, entry, little, tag, TYPE_ASCII, s.length, dataPos);
			bytes.set(s, dataPos);
			dataPos += s.length;
		}
	});
	view.setUint32(exifIfd + 2 + ascii.length * 12, 0, little);
	return bytes;
}

function writeEntry(
	view: DataView,
	pos: number,
	little: boolean,
	tag: number,
	type: number,
	count: number,
	value: number
) {
	view.setUint16(pos, tag, little);
	view.setUint16(pos + 2, type, little);
	view.setUint32(pos + 4, count, little);
	view.setUint32(pos + 8, value, little);
}
