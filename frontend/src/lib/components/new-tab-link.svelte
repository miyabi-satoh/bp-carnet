<script lang="ts">
	import type { Snippet } from 'svelte';
	import * as m from '$lib/paraglide/messages.js';

	let { href, children }: { href: string; children: Snippet } = $props();

	const LINK_CLASS = 'text-accent-foreground underline-offset-2 hover:underline';
</script>

<!-- 規約などを別のタブで開くリンク。見た目は変えず、読み上げでだけ別タブで開くと伝える。
     href は呼び出し側で resolve() 済みのパスを受け取る。 -->
<!-- eslint-disable-next-line svelte/no-navigation-without-resolve -->
<a {href} target="_blank" rel="noopener" class={LINK_CLASS}
	>{@render children()}<span class="sr-only">{m.common_opens_in_new_tab()}</span></a
>
