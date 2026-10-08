<script lang="ts">
	import type { Snippet } from 'svelte';
	import * as m from '$lib/paraglide/messages.js';
	import ExternalLinkIcon from '@lucide/svelte/icons/external-link';

	/** 使い方の本文から、詳しい手順を載せた外のサイトのページへのリンク。 */
	let { href, children }: { href: string; children: Snippet } = $props();
</script>

<!-- 外のサイトなので、読んでいる手順を閉じないよう新しいタブで開く。
     SvelteKit のルートではないので resolve() は使わない。 -->
<!-- eslint-disable svelte/no-navigation-without-resolve -->
<a
	{href}
	target="_blank"
	rel="noopener noreferrer"
	class="inline-flex min-h-6 items-center gap-1 self-start text-base text-accent-foreground underline underline-offset-4 hover:text-primary"
>
	{@render children()}
	<ExternalLinkIcon size={16} strokeWidth={2.2} aria-hidden="true" />
	<span class="sr-only">{m.common_opens_in_new_tab()}</span>
</a>
<!-- eslint-enable svelte/no-navigation-without-resolve -->
