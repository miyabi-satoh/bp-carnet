// テスト用: 「ヘルスケア」の書き出しの形の XML と zip を作る (apple-health.spec.ts と e2e で共通)。
import { deflateRawSync } from 'node:zlib';

export function record(type: string, startDate: string, value: string, unit = 'mmHg'): string {
	return ` <Record type="HKQuantityTypeIdentifier${type}" sourceName="OMRON connect" sourceVersion="1" device="&lt;&lt;HKDevice: 0x1&gt;, name:HEM&gt;" unit="${unit}" creationDate="${startDate}" startDate="${startDate}" endDate="${startDate}" value="${value}"/>\n`;
}

/** 「ヘルスケア」の書き出しの形。血圧は `<Correlation>` の中にも同じ `<Record>` が重ねて書かれる。 */
export function exportXml(body: string): string {
	return `<?xml version="1.0" encoding="UTF-8"?>\n<!DOCTYPE HealthData [\n<!ELEMENT HealthData (ExportDate,Me,(Record|Correlation)*)>\n]>\n<HealthData locale="ja_JP">\n <ExportDate value="2026-10-11 10:00:00 +0900"/>\n${body}</HealthData>\n`;
}

export type ZipFile = { name: string; content: string; stored?: boolean };

/** テスト用の最小の zip (ローカルヘッダー・セントラルディレクトリ・EOCD)。 */
export function zip(files: ZipFile[]): Uint8Array<ArrayBuffer> {
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
