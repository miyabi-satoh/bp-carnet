<script lang="ts">
	import { resolve } from '$app/paths';
	import { adjacentHelpTopics, setHelpTopicSlug } from '$lib/help';
	import * as m from '$lib/paraglide/messages.js';
	import { Button } from '$lib/components/ui/button';
	import PageHeader from '$lib/components/page-header.svelte';
	import ChevronLeftIcon from '@lucide/svelte/icons/chevron-left';
	import ChevronRightIcon from '@lucide/svelte/icons/chevron-right';
	import type { PageProps } from './$types';

	let { data }: PageProps = $props();

	let topic = $derived(data.topic);
	let adjacent = $derived(adjacentHelpTopics(topic));
	let Content = $derived(data.Content);

	setHelpTopicSlug(() => topic.slug);

	/** 前後の項目へのリンク。 */
	const ADJACENT_LINK_CLASS =
		'flex min-h-14 flex-col justify-center gap-0.5 rounded-lg raised bg-card px-4 py-2.5 transition-colors outline-none hover:bg-muted focus-visible:ring-3 focus-visible:ring-ring/50';
</script>

<!-- 使い方の項目。本文 ($lib/help/<言語>/<slug>.md) は、前置きと、1歩ずつの短い説明と画面の画像。 -->
<div class="mx-auto flex max-w-2xl flex-col px-5 pb-safe-8">
	<PageHeader backHref={resolve('/help')} title={topic.title()} />

	<div class="flex flex-col gap-4 text-base leading-relaxed">
		<Content />
	</div>

	<nav class="mt-6 flex flex-col gap-3">
		<div class="grid grid-cols-2 gap-3">
			{#if adjacent.prev}
				<a href={resolve('/help/[slug]', { slug: adjacent.prev.slug })} class={ADJACENT_LINK_CLASS}>
					<span class="flex items-center gap-1 text-sm text-muted-foreground">
						<ChevronLeftIcon size={16} strokeWidth={2.2} aria-hidden="true" />
						{m.help_prev_topic_label()}
					</span>
					<span class="text-base font-bold">{adjacent.prev.title()}</span>
				</a>
			{/if}
			{#if adjacent.next}
				<a
					href={resolve('/help/[slug]', { slug: adjacent.next.slug })}
					class={[ADJACENT_LINK_CLASS, 'col-start-2 items-end text-right']}
				>
					<span class="flex items-center gap-1 text-sm text-muted-foreground">
						{m.help_next_topic_label()}
						<ChevronRightIcon size={16} strokeWidth={2.2} aria-hidden="true" />
					</span>
					<span class="text-base font-bold">{adjacent.next.title()}</span>
				</a>
			{/if}
		</div>
		<Button href={resolve('/help')} variant="outline" class="w-full text-base">
			{m.help_back_to_index_button()}
		</Button>
	</nav>
</div>
