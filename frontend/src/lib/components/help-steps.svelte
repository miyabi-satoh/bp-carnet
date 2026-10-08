<script lang="ts">
	import type { Snippet } from 'svelte';

	/** 使い方の本文の手順の並び。中に `HelpStep` を置く。 */
	let { children }: { children: Snippet } = $props();
</script>

<ol class="flex flex-col gap-4">
	{@render children()}
</ol>
