<script lang="ts">
	import * as m from '$lib/paraglide/messages.js';

	/** 読み取りの枠1つの残りのゲージと割合 (docs/ocr.md)。写真で記録と設定の枠の一覧で使う。
	 * - `low` は全部の枠を足した残りが少ないとき。ゲージと割合を注意の色にする。
	 * - `exhausted` は全部の枠を使い切ったとき。橙の枠の中に置く前提で、ゲージの地を枠の色にする。 */
	let {
		percent,
		low = false,
		exhausted = false
	}: { percent: number; low?: boolean; exhausted?: boolean } = $props();
</script>

<div class="flex items-center gap-2.5">
	<div
		class={['h-2 flex-1 overflow-hidden rounded-full', exhausted ? 'bg-card' : 'bg-muted']}
		aria-hidden="true"
	>
		<div
			class={['h-full rounded-full', low ? 'bg-warn-foreground' : 'bg-primary']}
			style:width="{percent}%"
		></div>
	</div>
	<span
		class={[
			'text-xs font-bold whitespace-nowrap',
			exhausted || low ? 'text-warn-foreground' : 'text-muted-foreground'
		]}>{m.ocr_quota_remaining({ percent })}</span
	>
</div>
