<script lang="ts">
	import type { Snippet } from 'svelte';

	/** ページの下端に留める、主な操作のボタンの帯。 */
	let { children }: { children: Snippet } = $props();
</script>

<div
	data-slot="screen-footer"
	class="sticky bottom-0 flex shrink-0 gap-2.5 border-t bg-card px-5 pt-3 pb-safe-3"
>
	{@render children()}
</div>
