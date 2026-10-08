<script lang="ts">
	import { navigating } from '$app/state';
	import { delayedFlag, NAVIGATION_SHOW_DELAY_MS } from '$lib/delayed-flag.svelte';
	import * as m from '$lib/paraglide/messages.js';

	/** 画面遷移中のインジケーター。遷移のたびに `+layout.ts` が `/auth/me` を待つため、
	 * 遷移前の画面が残ったまま反応が無い時間ができる。それを画面上端の細いバーで示す。
	 *
	 * 中央のスピナーにしないのは、遷移先の内容と入れ替わるまでの短い時間で、画面を覆って
	 * しまわないようにするため。 */

	const visible = delayedFlag(() => navigating.to !== null, NAVIGATION_SHOW_DELAY_MS);
</script>

{#if visible.current}
	<div
		role="status"
		aria-label={m.common_loading()}
		class="fixed inset-x-0 top-safe z-50 h-1 overflow-hidden bg-primary/20 print:hidden"
	>
		<div class="h-full w-1/3 nav-progress-bar bg-primary"></div>
	</div>
{/if}
