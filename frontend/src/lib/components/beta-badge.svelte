<script lang="ts">
	import * as m from '$lib/paraglide/messages.js';

	/** 「オープンβテスト中」の小さなバッジ (`[server] beta_notice`、docs/architecture.md)。
	 * 色に意味を持たせず (朝・夜の色と衝突しないよう)、muted の中立色にする。 */
	let { class: className }: { class?: string } = $props();
</script>

<span
	class={[
		'inline-flex items-center rounded-full bg-muted px-2 py-0.5 text-xs font-bold text-muted-foreground',
		className
	]}
	role="img"
	aria-label={m.beta_badge_label()}
	title={m.beta_badge_label()}>β</span
>
