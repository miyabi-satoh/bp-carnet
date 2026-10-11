import { deflateRawSync } from 'node:zlib';
import { describe, expect, it } from 'vitest';
import { AppleHealthError, readAppleHealthExport } from './apple-health';

const TOKYO = 'Asia/Tokyo';

function record(type: string, startDate: string, value: string, unit = 'mmHg'): string {
	return ` <Record type="HKQuantityTypeIdentifier${type}" sourceName="OMRON connect" sourceVersion="1" device="&lt;&lt;HKDevice: 0x1&gt;, name:HEM&gt;" unit="${unit}" creationDate="${startDate}" startDate="${startDate}" endDate="${startDate}" value="${value}"/>\n`;
}

/** 「ヘルスケア」の書き出しの形。血圧は `<Correlation>` の中にも同じ `<Record>` が重ねて書かれる。 */
function exportXml(body: string): string {
	return `<?xml version="1.0" encoding="UTF-8"?>\n<!DOCTYPE HealthData [\n<!ELEMENT HealthData (ExportDate,Me,(Record|Correlation)*)>\n]>\n<HealthData locale="ja_JP">\n <ExportDate value="2026-10-11 10:00:00 +0900"/>\n${body}</HealthData>\n`;
}

const MORNING = '2026-02-05 07:08:33 +0900';
const EVENING = '2026-02-05 21:40:02 +0900';

const SAMPLE = exportXml(
	record('HeartRate', '2026-02-05 03:00:00 +0900', '55', 'count/min') +
		record('HeartRate', MORNING, '89', 'count/min') +
		record('HeartRateVariabilitySDNN', MORNING, '40', 'ms') +
		record('BloodPressureSystolic', EVENING, '132') +
		record('BloodPressureDiastolic', EVENING, '98') +
		record('BloodPressureSystolic', MORNING, '140') +
		record('BloodPressureDiastolic', MORNING, '106') +
		// 片方だけの記録は、血圧の記録にならない。
		record('BloodPressureSystolic', '2026-02-06 07:00:00 +0900', '128') +
		` <Correlation type="HKCorrelationTypeIdentifierBloodPressure" sourceName="OMRON connect" startDate="${MORNING}" endDate="${MORNING}">\n` +
		record('BloodPressureSystolic', MORNING, '140') +
		record('BloodPressureDiastolic', MORNING, '106') +
		' </Correlation>\n'
);

type ZipFile = { name: string; content: string; stored?: boolean };

/** テスト用の最小の zip (ローカルヘッダー・セントラルディレクトリ・EOCD)。 */
function zip(files: ZipFile[]): Uint8Array<ArrayBuffer> {
	const encoder = new TextEncoder();
	const chunks: Uint8Array[] = [];
	const central: Uint8Array[] = [];
	let offset = 0;
	for (const file of files) {
		const name = encoder.encode(file.name);
		const raw = encoder.encode(file.content);
		const data = file.stored ? raw : new Uint8Array(deflateRawSync(raw));
		const method = file.stored ? 0 : 8;

		const local = new DataView(new ArrayBuffer(30));
		local.setUint32(0, 0x04034b50, true);
		local.setUint16(8, method, true);
		local.setUint16(26, name.length, true);
		chunks.push(new Uint8Array(local.buffer), name, data);

		const entry = new DataView(new ArrayBuffer(46));
		entry.setUint32(0, 0x02014b50, true);
		entry.setUint16(10, method, true);
		entry.setUint32(20, data.length, true);
		entry.setUint32(24, raw.length, true);
		entry.setUint16(28, name.length, true);
		entry.setUint32(42, offset, true);
		central.push(new Uint8Array(entry.buffer), name);

		offset += 30 + name.length + data.length;
	}
	const centralSize = central.reduce((sum, part) => sum + part.length, 0);
	const eocd = new DataView(new ArrayBuffer(22));
	eocd.setUint32(0, 0x06054b50, true);
	eocd.setUint16(8, files.length, true);
	eocd.setUint16(10, files.length, true);
	eocd.setUint32(12, centralSize, true);
	eocd.setUint32(16, offset, true);

	const parts = [...chunks, ...central, new Uint8Array(eocd.buffer)];
	const bytes = new Uint8Array(parts.reduce((sum, part) => sum + part.length, 0));
	let pos = 0;
	for (const part of parts) {
		bytes.set(part, pos);
		pos += part.length;
	}
	return bytes;
}

const EXPECTED = [
	['2026-02-05', '07:08', '140', '106', '89'],
	['2026-02-05', '21:40', '132', '98', '']
];

function summary(rows: Awaited<ReturnType<typeof readAppleHealthExport>>) {
	return rows.map((row) => [row.measuredOnDate, row.time, row.systolic, row.diastolic, row.pulse]);
}

describe('readAppleHealthExport', () => {
	it('reads blood pressure from an exported zip, pairing the pulse measured at the same time', async () => {
		const file = new File(
			[
				zip([
					{ name: 'apple_health_export/export_cda.xml', content: '<ClinicalDocument/>' },
					{ name: 'apple_health_export/書き出したデータ.xml', content: SAMPLE }
				])
			],
			'書き出したデータ.zip'
		);
		const progress: number[] = [];
		const rows = await readAppleHealthExport(file, TOKYO, (fraction) => progress.push(fraction));
		expect(summary(rows)).toEqual(EXPECTED);
		expect(rows.every((row) => row.keep && !row.rowError)).toBe(true);
		expect(progress.at(-1)).toBe(1);
	});

	it('reads a stored (uncompressed) entry and a bare export.xml', async () => {
		const stored = new File(
			[zip([{ name: 'apple_health_export/export.xml', content: SAMPLE, stored: true }])],
			'export.zip'
		);
		expect(summary(await readAppleHealthExport(stored, TOKYO))).toEqual(EXPECTED);
		expect(summary(await readAppleHealthExport(new File([SAMPLE], 'export.xml'), TOKYO))).toEqual(
			EXPECTED
		);
	});

	it('finds records that straddle the chunks of a large file', async () => {
		// 流れの塊 (64KB ほど) の境目にタグがまたがるよう、長さのそろわない記録を大量に並べる。
		const filler = (i: number) =>
			record(
				'HeartRate',
				`2026-01-01 00:00:${String(i % 60).padStart(2, '0')} +0900`,
				String(i),
				'count/min'
			);
		const pairs = Array.from({ length: 3000 }, (_, i) => {
			const minute = String(i % 60).padStart(2, '0');
			const hour = String(Math.floor(i / 60) % 24).padStart(2, '0');
			const day = String(1 + Math.floor(i / 1440)).padStart(2, '0');
			const at = `2025-03-${day} ${hour}:${minute}:00 +0900`;
			return (
				filler(i) +
				record('BloodPressureSystolic', at, String(110 + (i % 30))) +
				record('BloodPressureDiastolic', at, String(70 + (i % 20)))
			);
		});
		const xml = exportXml(pairs.join(''));
		expect(xml.length).toBeGreaterThan(1_000_000);

		for (const file of [
			new File([xml], 'export.xml'),
			new File([zip([{ name: 'apple_health_export/export.xml', content: xml }])], 'export.zip')
		]) {
			const rows = await readAppleHealthExport(file, TOKYO);
			expect(rows).toHaveLength(3000);
			expect(rows[0]).toMatchObject({
				measuredOnDate: '2025-03-01',
				time: '00:00',
				systolic: '110'
			});
			expect(rows[2999]).toMatchObject({ systolic: String(110 + (2999 % 30)) });
		}
	});

	it('converts the time zone and kPa', async () => {
		const xml = exportXml(
			record('BloodPressureSystolic', '2026-02-05 23:30:00 +0000', '16.0', 'kPa') +
				record('BloodPressureDiastolic', '2026-02-05 23:30:00 +0000', '10.7', 'kPa')
		);
		expect(summary(await readAppleHealthExport(new File([xml], 'export.xml'), TOKYO))).toEqual([
			['2026-02-06', '08:30', '120', '80', '']
		]);
	});

	it('returns no rows when the export has no blood pressure', async () => {
		const xml = exportXml(record('HeartRate', MORNING, '89', 'count/min'));
		expect(await readAppleHealthExport(new File([xml], 'export.xml'), TOKYO)).toEqual([]);
	});

	it('rejects a zip that is not a Health export', async () => {
		const file = new File([zip([{ name: 'photos/a.txt', content: 'hello' }])], 'photos.zip');
		await expect(readAppleHealthExport(file, TOKYO)).rejects.toMatchObject({
			reason: 'not_export'
		} satisfies Partial<AppleHealthError>);
	});
});
