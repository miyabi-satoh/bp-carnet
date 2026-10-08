// OCR (写真からの読み取り) 用の、アップロード前のクライアント側画像加工。
//
// なぜリサイズするか: スマートフォンの写真は 3-4 MB / 4000px 級のことが多く、血圧計の
// 液晶を読み取るだけなら過剰な解像度。ブラウザ側でリサイズすることでアップロード時間・
// Gemini のトークンコスト・サーバー側のメモリ負荷を下げる。長辺 1600px / JPEG q=0.85 で
// 概ね 200-500KB に収まる (PoC での実測値を踏襲)。
//
// リサイズは EXIF の回転情報もピクセルデータへ焼き込む (Gemini 自体は回転した画像も
// 扱えるとされているが、一度正しい向きに補正しておけば余計な不確実性が無い)。

const DEFAULT_MAX_EDGE = 1600;
const DEFAULT_QUALITY = 0.85;

export type ResizeOptions = {
	maxEdge?: number;
	quality?: number;
	mimeType?: 'image/jpeg' | 'image/webp';
};

/** 画像ファイルをリサイズし、EXIF回転をピクセルデータに焼き込んだ `Blob` を返す。 */
export async function resizeImage(file: File, opts: ResizeOptions = {}): Promise<Blob> {
	const maxEdge = opts.maxEdge ?? DEFAULT_MAX_EDGE;
	const quality = opts.quality ?? DEFAULT_QUALITY;
	const mimeType = opts.mimeType ?? 'image/jpeg';

	// `{ imageOrientation: 'from-image' }` を指定すると EXIF の回転情報を尊重して
	// ビットマップを生成できる (=回転をピクセルに焼き込める)。
	const bitmap = await createImageBitmap(file, { imageOrientation: 'from-image' });
	try {
		const { width: srcW, height: srcH } = bitmap;
		const scale = Math.min(1, maxEdge / Math.max(srcW, srcH));
		const dstW = Math.round(srcW * scale);
		const dstH = Math.round(srcH * scale);

		const canvas =
			typeof OffscreenCanvas !== 'undefined'
				? new OffscreenCanvas(dstW, dstH)
				: Object.assign(document.createElement('canvas'), { width: dstW, height: dstH });
		const ctx = canvas.getContext('2d') as
			CanvasRenderingContext2D | OffscreenCanvasRenderingContext2D | null;
		if (!ctx) throw new Error('canvas 2d context unavailable');
		ctx.drawImage(bitmap, 0, 0, dstW, dstH);

		if (canvas instanceof OffscreenCanvas) {
			return await canvas.convertToBlob({ type: mimeType, quality });
		}
		return await new Promise<Blob>((resolve, reject) => {
			(canvas as HTMLCanvasElement).toBlob(
				(blob) => (blob ? resolve(blob) : reject(new Error('toBlob returned null'))),
				mimeType,
				quality
			);
		});
	} finally {
		bitmap.close?.();
	}
}
