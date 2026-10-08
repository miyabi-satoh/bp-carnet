<script lang="ts">
	import type { HTMLAttributes } from 'svelte/elements';
	import Maximize2Icon from '@lucide/svelte/icons/maximize-2';
	import * as m from '$lib/paraglide/messages.js';
	import { PhotoZoom } from '$lib/photo-zoom.svelte';
	import { cn } from '$lib/utils';
	import RoundButton from '$lib/components/round-button.svelte';

	/** 読み取りに使った写真の枠 (docs/ocr.md)。写真で記録のページと、行を直すシートで共通。枠の大きさは `class` で決める。
	 * 拡大率は写真を差し替えても戻さないので、写真ごとに作り直す (`{#key}`)。
	 * 枠の中で写真だけを拡大・移動でき、隅の拡大ボタンで全画面に出す。 */
	let {
		src,
		alt,
		onopen,
		startZoomed = false,
		class: className,
		...rest
	}: {
		src: string;
		alt: string;
		/** 全画面で出す。広がる動きの起点に、枠の大きさに切り取った写真の要素を渡す
		 * (写真そのものを渡すと、拡大した写真が枠の外へはみ出したまま動く)。 */
		onopen: (from: HTMLElement | undefined) => void;
		/** 最初からダブルタップと同じ倍率 (横幅いっぱいより少し大きく) で出すか。小さな枠では全体より一部を大きく
		 * 見せる方が読みやすいため。縮めれば全体も見られる。 */
		startZoomed?: boolean;
		class?: string;
	} & Omit<HTMLAttributes<HTMLDivElement>, 'class'> = $props();

	let view = $state<HTMLElement | null>(null);
	// 枠の上をなぞってページをスクロールさせるより写真を動かすことを優先し、指の操作はすべて受ける (touch-none)。
	// ホイールだけならページのスクロールに任せる (枠が画面の上に留まっていて、PC でページを送れなくなるため)。
	const zoom = new PhotoZoom({ clampToFrame: true, plainWheel: false, doubleTap: 'overflowWidth' });

	/** 写真を読み込み終えたら拡大する。読み込み済みならすぐ拡大する。 */
	function zoomInOnLoad(image: HTMLImageElement) {
		if (!startZoomed) return;
		const zoomIn = () => image.parentElement && zoom.zoomIn(image.parentElement);
		if (image.complete) {
			zoomIn();
			return;
		}
		image.addEventListener('load', zoomIn, { once: true });
		return () => image.removeEventListener('load', zoomIn);
	}
</script>

<div
	{...rest}
	class={cn('relative w-full shrink-0 overflow-hidden rounded-lg bg-muted', className)}
>
	<!-- 指の操作は写真を拡大・移動するだけで、キーボードでできる操作 (全画面) はボタンにある。 -->
	<div
		bind:this={view}
		role="group"
		class="absolute inset-0 touch-none overflow-hidden select-none"
		{...zoom.handlers}
	>
		<!-- 枠に対して絶対配置で中央に置く。flex の子にすると、WebKit (iOS Safari) ではシートを開き直したときに
		     max-h-full が効かず、写真が枠からはみ出す。 -->
		<img
			{@attach zoomInOnLoad}
			{src}
			{alt}
			draggable="false"
			class={[
				'absolute inset-0 m-auto max-h-full max-w-full',
				zoom.animate && 'transition-transform duration-300 ease-out motion-reduce:transition-none'
			]}
			style:transform={zoom.transform}
		/>
	</div>
	<span
		class="pointer-events-none absolute top-2.5 right-2.5 rounded-full bg-foreground/70 px-2.5 py-1 text-xs text-background"
	>
		{m.photo_page_preview_badge()}
	</span>
	<!-- 写真の上に重ねるので、地と文字の色を写真から浮く組み合わせにする (ホバーしても文字色を変えない)。 -->
	<RoundButton
		class="absolute right-2 bottom-2 bg-foreground/70 text-background hover:text-background"
		aria-label={m.photo_page_preview_open_label()}
		onclick={() => onopen(view ?? undefined)}
	>
		<Maximize2Icon size={20} strokeWidth={2.2} aria-hidden="true" />
	</RoundButton>
</div>
