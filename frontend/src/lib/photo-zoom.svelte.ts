/** 写真だけを拡大・移動する操作 (docs/ocr.md)。全画面表示 (photo-lightbox.svelte) と、画面の中の写真の枠
 * (photo-preview.svelte) で共通。ピンチ・ダブルタップ・ホイールで拡大し、拡大中はドラッグで動かす。
 * 拡大は指やカーソルの位置を中心にする (枠の中央を中心にすると、小さな枠では見たい所が枠の外へ出ていくため)。
 *
 * 使い方: `handlers` を写真 (`img`) を囲む要素に付け、写真の要素に `transform` を当てる。 */

/** 拡大の上限。横幅いっぱい (写真が横幅に収まっていれば等倍) からの倍率で数える。低い枠では横幅いっぱいにするだけで
 * 数倍になるため、等倍から数えると横幅いっぱいまでも届かない。 */
const MAX_SCALE = 5;
/** 全画面でダブルタップしたときの倍率の下限 (→ `#doubleTapScale`)。 */
const DOUBLE_TAP_SCALE = 2.5;
/** 枠の中でダブルタップしたときの、横幅いっぱいに対する倍率。左右が少しはみ出すくらいが読みやすい (2026-09-18 にユーザーが実機で判断)。 */
const OVERFLOW_WIDTH_RATE = 1.2;
/** 2回のタップを1回のダブルタップとみなす間隔 (ms) と、指が動いてよい距離 (px)。 */
const DOUBLE_TAP_MS = 300;
const TAP_SLOP = 10;

type Point = { x: number; y: number };

type PhotoZoomOptions = {
	/** 写真が枠の外へ逃げないよう、動かせる範囲を写真の端までにするか。 */
	clampToFrame: boolean;
	/** ホイールだけで拡大するか。`false` なら Ctrl + ホイール (トラックパッドのピンチ) だけで拡大し、
	 * ホイールだけならページのスクロールに任せる。 */
	plainWheel: boolean;
	/** ダブルタップで拡大する倍率の決め方。`fixed` は「横幅いっぱいか決まった倍率の大きいほう」(全画面)、
	 * `overflowWidth` は「横幅いっぱいより少し大きく」(画面の中の小さな枠)。 */
	doubleTap: 'fixed' | 'overflowWidth';
};

export class PhotoZoom {
	scale = $state(1);
	offsetX = $state(0);
	offsetY = $state(0);
	/** 拡大・移動をアニメーションで見せるか。ダブルタップのときだけ付け、指やホイールで動かす間は外す (指に遅れて付いてこないように)。 */
	animate = $state(false);

	readonly #options: PhotoZoomOptions;

	/** 画面に触れている指 (マウスを含む) の今の位置。計算にだけ使い、画面には出さないので反応性は要らない。 */
	// eslint-disable-next-line svelte/prefer-svelte-reactivity
	readonly #pointers = new Map<number, Point>();
	/** 2本指で触れ始めたときの、指の間隔と中点と倍率。 */
	#pinchStart: { distance: number; mid: Point; scale: number } | null = null;
	/** 1本指で触れ始めた位置 (タップかどうかを見分ける)。 */
	#tapStart: Point | null = null;
	/** 直前のタップの時刻。無ければ null (ダブルタップの2回目として数えない)。 */
	#lastTapAt: number | null = null;

	constructor(options: PhotoZoomOptions) {
		this.#options = options;
	}

	get transform() {
		return `translate(${this.offsetX}px, ${this.offsetY}px) scale(${this.scale})`;
	}

	/** 等倍に戻し、触れている指の記録も片付ける (指を置いたまま閉じると、その指の pointerup は届かないため)。 */
	reset() {
		this.animate = false;
		this.#resetZoom();
		this.#pointers.clear();
		this.#pinchStart = null;
		this.#tapStart = null;
		this.#lastTapAt = null;
	}

	/** ダブルタップと同じ倍率まで、写真の中央を中心に拡大する。 */
	zoomIn(frame: HTMLElement) {
		this.animate = false;
		this.#resetZoom();
		this.scale = this.#doubleTapScale(frame);
	}

	/** 写真を枠の横幅いっぱいにする倍率。測れなければ 1。 */
	#fillScale(frame: HTMLElement) {
		const image = frame.querySelector('img');
		if (!image || image.offsetWidth === 0) return 1;
		return frame.clientWidth / image.offsetWidth;
	}

	#maxScale(frame: HTMLElement) {
		return MAX_SCALE * Math.max(1, this.#fillScale(frame));
	}

	/** ダブルタップで拡大する倍率。`zoomIn` で開いた枠は、ダブルタップで最初の見え方に戻れる。 */
	#doubleTapScale(frame: HTMLElement) {
		const fill = this.#fillScale(frame);
		const scale =
			this.#options.doubleTap === 'fixed'
				? Math.max(DOUBLE_TAP_SCALE, fill)
				: Math.max(1, fill) * OVERFLOW_WIDTH_RATE;
		return Math.min(this.#maxScale(frame), scale);
	}

	readonly handlers = {
		onpointerdown: (event: PointerEvent) => {
			(event.currentTarget as HTMLElement).setPointerCapture(event.pointerId);
			this.#pointers.set(event.pointerId, { x: event.clientX, y: event.clientY });
			if (this.#pointers.size >= 2) {
				// 指が増えるたびに取り直す (3本目を置いてから1本離すと、測る2本の組み合わせが変わるため)。
				this.#startPinch();
				this.#tapStart = null;
				this.#lastTapAt = null;
			} else if (this.#pointers.size === 1) {
				this.#tapStart = { x: event.clientX, y: event.clientY };
			}
		},
		onpointermove: (event: PointerEvent) => {
			const previous = this.#pointers.get(event.pointerId);
			if (!previous) return;
			this.#pointers.set(event.pointerId, { x: event.clientX, y: event.clientY });
			const frame = event.currentTarget as HTMLElement;
			if (this.#pointers.size >= 2 && this.#pinchStart) {
				this.animate = false;
				const start = this.#pinchStart;
				const mid = this.#mid();
				// 指の中点の下にある所を中点に付いて動かし、そこを中心に拡大する。
				this.#pan(frame, mid.x - start.mid.x, mid.y - start.mid.y);
				this.#pinchStart = { ...start, mid };
				this.#zoomAt(frame, (start.scale * this.#distance()) / start.distance, mid);
			} else if (this.#pointers.size === 1 && this.scale > 1) {
				this.animate = false;
				this.#pan(frame, event.clientX - previous.x, event.clientY - previous.y);
			}
		},
		onpointerup: (event: PointerEvent) => {
			this.#release(event);
			const tapStart = this.#tapStart;
			if (!tapStart || this.#pointers.size > 0) return;
			const moved = Math.hypot(event.clientX - tapStart.x, event.clientY - tapStart.y);
			this.#tapStart = null;
			// 動かした操作を挟んだら、前のタップとは続けて数えない。
			if (moved > TAP_SLOP) {
				this.#lastTapAt = null;
				return;
			}
			if (this.#lastTapAt !== null && event.timeStamp - this.#lastTapAt < DOUBLE_TAP_MS) {
				this.animate = true;
				if (this.scale > 1) this.#resetZoom();
				else
					this.#zoomAt(
						event.currentTarget as HTMLElement,
						this.#doubleTapScale(event.currentTarget as HTMLElement),
						{
							x: event.clientX,
							y: event.clientY
						}
					);
				this.#lastTapAt = null;
			} else {
				this.#lastTapAt = event.timeStamp;
			}
		},
		onpointercancel: (event: PointerEvent) => this.#cancel(event),
		/** 捕捉は指を離したとき (`pointerup` の後) にも外れる。そのときは片付け済みなので、タップの記録を残す。 */
		onlostpointercapture: (event: PointerEvent) => {
			if (this.#pointers.has(event.pointerId)) this.#cancel(event);
		},
		onwheel: (event: WheelEvent) => {
			if (!this.#options.plainWheel && !event.ctrlKey) return;
			event.preventDefault();
			this.animate = false;
			// トラックパッドのピンチ (ctrlKey) は1回あたりの量が小さいので、よく効かせる。
			const rate = event.ctrlKey ? 0.01 : 0.002;
			this.#zoomAt(
				event.currentTarget as HTMLElement,
				this.scale * Math.exp(-event.deltaY * rate),
				{
					x: event.clientX,
					y: event.clientY
				}
			);
		}
	};

	#resetZoom() {
		this.scale = 1;
		this.offsetX = 0;
		this.offsetY = 0;
	}

	/** `at` (画面の座標) の下にある所を動かさずに、倍率を `next` にする。 */
	#zoomAt(frame: HTMLElement, next: number, at: Point) {
		const scale = Math.min(this.#maxScale(frame), Math.max(1, next));
		if (scale === 1) {
			this.#resetZoom();
			return;
		}
		// 変形の中心は、動かす前の写真の中央。今の写真の中央から、動かした分を差し引いて求める。
		const rect = (frame.querySelector('img') ?? frame).getBoundingClientRect();
		const cx = rect.left + rect.width / 2 - this.offsetX;
		const cy = rect.top + rect.height / 2 - this.offsetY;
		const ratio = scale / this.scale;
		this.offsetX = at.x - cx - (at.x - cx - this.offsetX) * ratio;
		this.offsetY = at.y - cy - (at.y - cy - this.offsetY) * ratio;
		this.scale = scale;
		this.#clamp(frame);
	}

	#pan(frame: HTMLElement, dx: number, dy: number) {
		this.offsetX += dx;
		this.offsetY += dy;
		this.#clamp(frame);
	}

	#clamp(frame: HTMLElement) {
		if (!this.#options.clampToFrame) return;
		const image = frame.querySelector('img');
		if (!image) return;
		// 写真の端が枠の端を越えて内側へ入らない範囲。写真が枠より小さい向きは動かさない。
		const maxX = Math.max(0, (image.offsetWidth * this.scale - frame.clientWidth) / 2);
		const maxY = Math.max(0, (image.offsetHeight * this.scale - frame.clientHeight) / 2);
		this.offsetX = Math.min(maxX, Math.max(-maxX, this.offsetX));
		this.offsetY = Math.min(maxY, Math.max(-maxY, this.offsetY));
	}

	#distance() {
		const [a, b] = [...this.#pointers.values()];
		return Math.hypot(a.x - b.x, a.y - b.y);
	}

	#mid(): Point {
		const [a, b] = [...this.#pointers.values()];
		return { x: (a.x + b.x) / 2, y: (a.y + b.y) / 2 };
	}

	#startPinch() {
		this.#pinchStart = { distance: this.#distance(), mid: this.#mid(), scale: this.scale };
	}

	/** 指が離れた・取り消された・捕捉が外れたとき。タップかどうかは見ない。 */
	#release(event: PointerEvent) {
		this.#pointers.delete(event.pointerId);
		if (this.#pointers.size >= 2) this.#startPinch();
		else this.#pinchStart = null;
	}

	#cancel(event: PointerEvent) {
		this.#release(event);
		this.#tapStart = null;
		this.#lastTapAt = null;
	}
}
