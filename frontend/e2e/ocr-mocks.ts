// 写真の読み取り (Gemini) を呼ばずに、読み取りの画面を流す。使い方のページの手順 (→ help.e2e.ts) で使う。
// 本物の Gemini で流すシナリオ (`@ocr`) とは別物で、API キーの無い普段の実行でも流せる。
// 仕様書のスクリーンショット (scripts/shots.ts) も使い、Node で直接読むので、相対 import は `.ts` まで書く。
import type { Page } from '@playwright/test';
import type { components } from '../src/lib/api/schema';
import { withExif, type ExifFields } from '../src/lib/photo-taken-at.test-data.ts';
import { drawInBlankPage } from './helpers.ts';

type OcrResult = components['schemas']['OcrResult'];
type OcrReading = components['schemas']['OcrReading'];
type OcrStatusResponse = components['schemas']['OcrStatusResponse'];
type OcrQuota = components['schemas']['OcrQuotaResponse'];

/** 無料の読み取りの枠。 */
export function freeQuota(remainingPercent: number): OcrQuota {
	return { kind: 'free', purchasedAt: null, remainingPercent };
}

/** 買い足した読み取りの枠。 */
export function paidQuota(remainingPercent: number, purchasedAt: string): OcrQuota {
	return { kind: 'paid', purchasedAt, remainingPercent };
}

/** 読み取りの応答の信頼度。「高」にして、「要確認」を出さない。 */
const SAMPLE_CONFIDENCE = 0.95;

/** 読み取りの状態 (`GET /ocr/status`) を、写真で記録を出せる既定の状態に `overrides` をかぶせて答える。ページを開く前に呼ぶ。
 * 既定は、有効・同意済み・枠なし (無制限)・買い足せない (ウェブ・アプリとも)。 */
export async function mockOcrStatus(
	page: Page,
	overrides: Partial<OcrStatusResponse> = {}
): Promise<void> {
	const body: OcrStatusResponse = {
		enabled: true,
		quotas: null,
		quotaLow: false,
		topupAvailable: false,
		appStoreTopupAvailable: false,
		consented: true,
		...overrides
	};
	await page.route('**/api/v1/ocr/status', (route) => route.fulfill({ json: body }));
}

/** 写真の読み取り (`POST /ocr`) の応答を差し替える。呼び直すと、後の応答に替わる。 */
export async function mockOcrExtract(
	page: Page,
	response: { status?: number; json: unknown }
): Promise<void> {
	await page.unroute('**/api/v1/ocr');
	await page.route('**/api/v1/ocr', (route) =>
		route.request().method() === 'POST' ? route.fulfill(response) : route.continue()
	);
}

const NO_USAGE = { inputTokens: 0, outputTokens: 0 };

/** 血圧計の液晶の読み取り結果。 */
export function monitorResult(values: { systolic: number; diastolic: number; pulse: number }) {
	const result: OcrResult = {
		kind: 'monitor_bp',
		reading: { ...values, confidence: SAMPLE_CONFIDENCE },
		usage: NO_USAGE
	};
	return result;
}

/** 血圧値の写っていない写真の読み取り結果。 */
export function noValuesResult(): OcrResult {
	return { kind: 'monitor_none', usage: NO_USAGE };
}

export type NotebookRow = {
	/** `YYYY-MM-DD`。 */
	date: string;
	/** `HH:MM`。省くと、時刻の書かれていない行になる。 */
	time?: string;
	systolic: number;
	diastolic: number;
	pulse: number;
	/** 行に添えて書かれた文章。 */
	memo?: string;
	/** 省くと「高」。 */
	confidence?: number;
	/** 行に書かれた朝・夜。省くと書かれていない行。 */
	period?: 'morning' | 'evening';
};

/** 手帳の複数行の読み取り結果。 */
export function notebookResult(rows: readonly NotebookRow[]): OcrResult {
	const readings = rows.map((row): OcrReading => ({
		measuredOn: row.date.slice(5),
		time: row.time ?? null,
		systolic: row.systolic,
		diastolic: row.diastolic,
		pulse: row.pulse,
		memo: row.memo,
		confidence: row.confidence ?? SAMPLE_CONFIDENCE,
		period: row.period
	}));
	return { kind: 'memo', readings, usage: NO_USAGE };
}

/** API のエラーの応答 (→ src/error.rs の ErrorResponse)。 */
export function errorResponse(status: number, code: string) {
	return { status, json: { error: { code, message: code } } };
}

/**
 * ファイルの選択に渡す見本の写真 (PNG)。実在の人や記録が写らないよう、その場で描く。
 * `monitor` は血圧計の液晶、`notebook` は手帳のページ。
 */
export async function samplePhoto(
	page: Page,
	photo:
		/** `values` を省くと、数字の出ていない液晶になる。 */
		| { kind: 'monitor'; values?: { systolic: number; diastolic: number; pulse: number } }
		| { kind: 'notebook'; rows: readonly NotebookRow[] },
	/** 渡すと、この撮影日時の Exif を持つ JPEG にする (Exif は JPEG にしか付けられないため)。 */
	exif?: ExifFields
): Promise<{ name: string; mimeType: string; buffer: Buffer }> {
	const mimeType = exif ? 'image/jpeg' : 'image/png';
	const drawn = await drawInBlankPage(
		page,
		mimeType,
		({ photo, mimeType }) => {
			const canvas = document.createElement('canvas');
			canvas.width = 600;
			canvas.height = 800;
			const c = canvas.getContext('2d');
			if (!c) throw new Error('canvas の 2D コンテキストを取れなかった');
			if (photo.kind === 'monitor') {
				c.fillStyle = '#cfc9bd';
				c.fillRect(0, 0, 600, 800);
				c.fillStyle = '#f4f4f2';
				c.beginPath();
				c.roundRect(70, 60, 460, 680, 40);
				c.fill();
				c.fillStyle = '#b9c2b4';
				c.fillRect(110, 110, 380, 480);
				c.fillStyle = '#20241f';
				c.textAlign = 'right';
				c.textBaseline = 'alphabetic';
				c.font = 'bold 150px sans-serif';
				c.fillText(String(photo.values?.systolic ?? '--'), 400, 250);
				c.fillText(String(photo.values?.diastolic ?? '--'), 400, 410);
				c.font = 'bold 110px sans-serif';
				c.fillText(String(photo.values?.pulse ?? '--'), 400, 555);
				c.textAlign = 'left';
				c.font = '28px sans-serif';
				c.fillText('SYS', 410, 250);
				c.fillText('DIA', 410, 410);
				c.fillText('PUL', 410, 555);
			} else {
				c.fillStyle = '#9aa3ad';
				c.fillRect(0, 0, 600, 800);
				c.fillStyle = '#fdfcf7';
				c.fillRect(60, 40, 480, 720);
				c.strokeStyle = '#b8cbe0';
				c.lineWidth = 2;
				for (let y = 140; y < 760; y += 80) {
					c.beginPath();
					c.moveTo(60, y);
					c.lineTo(540, y);
					c.stroke();
				}
				c.fillStyle = '#2b2f3a';
				c.font = 'italic 44px cursive';
				photo.rows.forEach((row, i) => {
					const [, month, day] = row.date.split('-').map(Number);
					const values = `${row.systolic}  ${row.diastolic}  ${row.pulse}`;
					c.fillText(`${month}/${day}`, 90, 125 + i * 80);
					c.fillText(values, 230, 125 + i * 80);
				});
			}
			return canvas.toDataURL(mimeType);
		},
		{ photo, mimeType }
	);
	const buffer = exif ? Buffer.from(withExif(drawn, exif)) : drawn;
	return { name: `${photo.kind}.${exif ? 'jpg' : 'png'}`, mimeType, buffer };
}
