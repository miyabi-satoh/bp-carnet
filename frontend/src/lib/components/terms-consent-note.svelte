<script lang="ts">
	import { resolve } from '$app/paths';
	import * as m from '$lib/paraglide/messages.js';
	import NewTabLink from '$lib/components/new-tab-link.svelte';

	/** 登録するボタンの直上に置く、利用規約・プライバシーポリシーへの同意の文 (docs/authentication.md)。 */
	let {
		message
	}: {
		/** `{terms}`・`{privacy}` の差し込み口を持つ文言。差し込み口はリンクにする。 */
		message: (inputs: { terms: string; privacy: string }) => string;
	} = $props();

	/** 文言を差し込み口で切り分けるための印。文言には現れない文字にする。 */
	const MARK = '␟';

	const parts = $derived(
		message({ terms: `${MARK}terms${MARK}`, privacy: `${MARK}privacy${MARK}` }).split(MARK)
	);
</script>

<!-- 規約は別のタブで開く。入力中のフォームを離れずに読めるように。見た目は変えず、読み上げでだけそう伝える。 -->
<p class="text-xs leading-relaxed text-muted-foreground">
	{#each parts as part, i (i)}
		<!-- 「プライバシー｜ポリシー」のように、リンクの語の途中で折り返して読みにくくならないようにする。 -->
		{#if part === 'terms'}
			<span class="whitespace-nowrap"
				><NewTabLink href={resolve('/terms')}>{m.terms_title()}</NewTabLink></span
			>
		{:else if part === 'privacy'}
			<span class="whitespace-nowrap"
				><NewTabLink href={resolve('/privacy')}>{m.privacy_title()}</NewTabLink></span
			>
		{:else}
			{part}
		{/if}
	{/each}
</p>
