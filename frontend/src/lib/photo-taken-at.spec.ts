import { describe, expect, it } from 'vitest';
import { parseExifDateTime, photoTakenAt } from './photo-taken-at';
import { withExif, type ExifFields } from './photo-taken-at.test-data';

/** SOI と EOI だけの JPEG。読むのは画像データより前の Exif だけなので足りる。 */
const EMPTY_JPEG = new Uint8Array([0xff, 0xd8, 0xff, 0xd9]);

function jpeg(fields: ExifFields): Blob {
	return new Blob([withExif(EMPTY_JPEG, fields)], { type: 'image/jpeg' });
}

function parse(fields: ExifFields) {
	const bytes = withExif(EMPTY_JPEG, fields);
	return parseExifDateTime(bytes.buffer);
}

const TZ = 'Asia/Tokyo';
const NOW = new Date('2026-09-18T10:00:00+09:00');

describe('parseExifDateTime', () => {
	it('reads DateTimeOriginal in both byte orders', () => {
		for (const littleEndian of [false, true]) {
			expect(parse({ dateTimeOriginal: '2026:09:17 07:15:42', littleEndian })).toEqual({
				local: '2026-09-17T07:15',
				offset: null
			});
		}
	});

	it('reads the offset when present', () => {
		expect(
			parse({ dateTimeOriginal: '2026:09:17 07:15:00', offsetTimeOriginal: '+09:00' })
		).toEqual({ local: '2026-09-17T07:15', offset: '+09:00' });
	});

	it('falls back to DateTimeDigitized', () => {
		expect(parse({ dateTimeDigitized: '2026:09:16 21:30:00' })?.local).toBe('2026-09-16T21:30');
	});

	it('treats an unset or impossible date as missing', () => {
		expect(parse({ dateTimeOriginal: '0000:00:00 00:00:00' })).toBeNull();
		expect(parse({ dateTimeOriginal: '2026:02:30 07:00:00' })).toBeNull();
		expect(parse({ dateTimeOriginal: '2026-09-17 07:00:00' })).toBeNull();
	});

	it('returns null for data without Exif, non-JPEG and truncated Exif', () => {
		expect(parseExifDateTime(EMPTY_JPEG.buffer as ArrayBuffer)).toBeNull();
		expect(parseExifDateTime(new Uint8Array([0x89, 0x50, 0x4e, 0x47]).buffer)).toBeNull();
		const full = withExif(EMPTY_JPEG, { dateTimeOriginal: '2026:09:17 07:15:00' });
		expect(parseExifDateTime(full.slice(0, 40).buffer)).toBeNull();
	});
});

describe('photoTakenAt', () => {
	it('uses the camera clock as the user time zone when there is no offset', async () => {
		expect(await photoTakenAt(jpeg({ dateTimeOriginal: '2026:09:17 07:15:00' }), TZ, NOW)).toBe(
			'2026-09-17T07:15'
		);
	});

	it('converts to the user time zone when the offset is known', async () => {
		const photo = jpeg({ dateTimeOriginal: '2026:09:16 23:15:00', offsetTimeOriginal: '+01:00' });
		expect(await photoTakenAt(photo, TZ, NOW)).toBe('2026-09-17T07:15');
	});

	it('ignores dates in the future (a wrong camera clock)', async () => {
		expect(
			await photoTakenAt(jpeg({ dateTimeOriginal: '2026:09:18 10:01:00' }), TZ, NOW)
		).toBeNull();
		const offset = jpeg({ dateTimeOriginal: '2026:09:18 02:00:00', offsetTimeOriginal: '+00:00' });
		expect(await photoTakenAt(offset, TZ, NOW)).toBeNull();
		expect(await photoTakenAt(jpeg({ dateTimeOriginal: '2026:09:18 10:00:00' }), TZ, NOW)).toBe(
			'2026-09-18T10:00'
		);
	});

	it('returns null when the photo has no date', async () => {
		expect(await photoTakenAt(new Blob([EMPTY_JPEG]), TZ, NOW)).toBeNull();
	});
});
