<script lang="ts">
	import type { Snippet } from 'svelte';
	import { asset } from '$app/paths';
	import { getHelpTopicSlug, HELP_STEP_IMAGE_SIZE, helpStepImagePath } from '$lib/help';
	import * as m from '$lib/paraglide/messages.js';

	/** 使い方の本文 (`$lib/help/<言語>/<slug>.md`) の1歩。`n` は 1 始まりの番号で、画像のファイル名にも使う。
	 * ブラウザのメニューなどアプリの外の操作は画像を撮れないため、`image={false}` にする。 */
	let { n, image = true, children }: { n: number; image?: boolean; children: Snippet } = $props();

	const slug = getHelpTopicSlug();
</script>

<li class="flex flex-col gap-4 rounded-lg raised bg-card p-4">
	<div class="flex items-start gap-3">
		<span
			class="flex size-8 shrink-0 items-center justify-center rounded-full bg-primary text-base font-bold text-primary-foreground"
			aria-hidden="true">{n}</span
		>
		<div class="flex flex-col gap-2 pt-0.5 text-base leading-relaxed">
			{@render children()}
		</div>
	</div>
	{#if image}
		<!-- 画面全体の画像で縦に長いため、幅を抑えて1歩が1画面に収まりやすくする。 -->
		<img
			src={asset(helpStepImagePath(slug(), n))}
			alt={m.help_step_image_alt({ number: n })}
			width={HELP_STEP_IMAGE_SIZE.width}
			height={HELP_STEP_IMAGE_SIZE.height}
			loading="lazy"
			class="mx-auto h-auto w-full max-w-64 rounded-md border"
		/>
	{/if}
</li>
