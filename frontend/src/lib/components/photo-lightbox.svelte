<script lang="ts">
	import { tick } from 'svelte';
	import XIcon from '@lucide/svelte/icons/x';
	import * as Dialog from '$lib/components/ui/dialog';
	import { HistoryOverlay } from '$lib/history-overlay.svelte';
	import * as m from '$lib/paraglide/messages.js';
	import { PhotoZoom } from '$lib/photo-zoom.svelte';

	/** 写真を全画面で見るダイアログ。ピンチ・ダブルタップ・ホイールで拡大し、
	 * 拡大中はドラッグで動かす。開くと `pushState` で履歴を積み、端末の戻る操作でも閉じる。
	 * 開く・ボタンや Esc で閉じるときは、View Transitions API で元の写真との間を行き来させる
	 * (未対応のブラウザ・動きを減らす設定ではそのまま切り替える。戻る操作はブラウザが先に履歴を戻すので包めない)。
	 *
	 * ADR: 拡大はブラウザのページ拡大に任せず、写真だけを CSS の transform で拡大する。ページごと
	 * 拡大すると閉じるボタンや案内まで画面の外に出て、戻し方が分からなくなるため。 */
	let {
		src,
		alt
	}: {
		src: string;
		alt: string;
	} = $props();

	/** 元の写真と全画面の写真を、ひとつの写真として動かすための名前。 */
	const TRANSITION_NAME = 'photo-lightbox';

	const zoom = new PhotoZoom({ clampToFrame: false, plainWheel: true, doubleTap: 'fixed' });
	/** 開くときに履歴へ印を積み、戻る操作でその印から出ると閉じる。 */
	const overlay = new HistoryOverlay('photoLightbox');
	/** 開いたときの元の写真。閉じるときにそこへ戻す。 */
	let source: HTMLElement | undefined;

	function canTransition() {
		return (
			typeof document.startViewTransition === 'function' &&
			!matchMedia('(prefers-reduced-motion: reduce)').matches
		);
	}

	/** `from` を渡すと、その写真から全画面へ広がって開く。 */
	export function show(from?: HTMLElement) {
		if (closing) return;
		source = from;
		if (!from || !canTransition()) {
			pushOpenState();
			return;
		}
		from.style.viewTransitionName = TRANSITION_NAME;
		document.startViewTransition(async () => {
			from.style.viewTransitionName = '';
			pushOpenState();
			await tick();
		});
	}

	function pushOpenState() {
		zoom.reset();
		overlay.show();
	}

	/** 閉じている途中か (戻した履歴が page.state に届き、閉じる動きが終わるまで)。途中で続けて閉じるとページまで戻り、
	 * 開き直すと閉じる動きと重なるため、その間の操作は受けない。$state にしないのは、閉じ終わっても
	 * `overlay` の effect を走り直させないため (閉じている最中はすでに印を戻した後なので、effect は戻らない)。 */
	let closing = false;

	function close() {
		if (!overlay.marked || closing) return;
		closing = true;
		const popped = new Promise<void>((resolve) =>
			addEventListener('popstate', () => resolve(), { once: true })
		);
		const to = source;
		if (!to || !canTransition()) {
			popped.then(() => {
				closing = false;
			});
			overlay.back();
			return;
		}
		const transition = document.startViewTransition(async () => {
			// 戻った履歴を SvelteKit が page.state に反映してから、閉じた後の画面を撮らせる。
			overlay.back();
			await popped;
			await tick();
			to.style.viewTransitionName = TRANSITION_NAME;
		});
		transition.finished.finally(() => {
			to.style.viewTransitionName = '';
			closing = false;
		});
	}
</script>

<Dialog.Root
	bind:open={
		() => overlay.open,
		(value) => {
			if (!value) close();
		}
	}
>
	<!-- 画面いっぱいに出す。Dialog.Content 既定の中央寄せ・幅・角丸・余白を打ち消している。
	     開くときの拡大の動きは消す (写真が元の位置から広がる動きと重なるため)。
	     横向きのノッチの下に閉じるボタンが潜らないよう、左右の safe area を避ける (body の余白は固定配置に効かない)。 -->
	<Dialog.Content
		class="top-0 left-0 flex h-dvh max-w-none translate-x-0 translate-y-0 flex-col gap-0 rounded-none border-none bg-neutral-950 p-0 px-safe text-white ring-0 sm:max-w-none data-open:animate-none"
	>
		<Dialog.Title class="sr-only">{alt}</Dialog.Title>
		<div class="flex shrink-0 justify-end p-5 pt-safe-5">
			<Dialog.Close
				class="inline-flex h-11 items-center gap-1.5 rounded-full bg-white/15 pr-4 pl-3 text-base font-bold outline-none hover:bg-white/25 focus-visible:ring-3 focus-visible:ring-white/50"
			>
				<XIcon size={18} strokeWidth={2.4} aria-hidden="true" />
				{m.common_close_button()}
			</Dialog.Close>
		</div>
		<!-- 指の操作は写真を拡大・移動するだけで、キーボードでできる操作 (閉じる) はボタンにある。 -->
		<div
			role="group"
			class="flex min-h-0 flex-1 touch-none items-center justify-center overflow-hidden select-none"
			{...zoom.handlers}
		>
			<img
				{src}
				{alt}
				draggable="false"
				style:view-transition-name={overlay.open ? TRANSITION_NAME : undefined}
				class={[
					'max-h-full max-w-full object-contain',
					// 閉じる間の薄れる動きに写真を残さない (閉じた先の写真へ動いて戻る写真と二重に見えるため)。
					!overlay.open && 'invisible',
					zoom.animate && 'transition-transform duration-300 ease-out motion-reduce:transition-none'
				]}
				style:transform={zoom.transform}
			/>
		</div>
		<p class="shrink-0 p-5 pb-safe-8 text-center text-sm text-white/75">
			{m.photo_lightbox_hint()}
		</p>
	</Dialog.Content>
</Dialog.Root>
