<script lang="ts">
	import type { Snippet } from 'svelte';

	/** ページの中で横に並べるボタンの組。控えめな操作を左、主な操作を右に置く。
	 * ボタンは同じ幅に伸ばし、文言が横に収まらない狭い画面 (iPhone SE など) では2段に折り返す。
	 * 画面の下端に留める帯は `screen-footer.svelte`。 */
	let { children, class: className }: { children: Snippet; class?: string } = $props();
</script>

<div class={['flex flex-wrap gap-2 *:flex-1', className]}>
	{@render children()}
</div>
