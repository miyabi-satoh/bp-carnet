<script lang="ts">
	import { resolve } from '$app/paths';
	import { HELP_SECTIONS } from '$lib/help';
	import * as m from '$lib/paraglide/messages.js';
	import { LIST_ROW_CLASS, SECTION_TITLE_CLASS } from '$lib/styles';
	import PageHeader from '$lib/components/page-header.svelte';
	import ChevronRightIcon from '@lucide/svelte/icons/chevron-right';

	const sections = HELP_SECTIONS.filter((section) => section.topics.length > 0);
</script>

<!-- 使い方の目次。見た目は設定画面の一覧に揃える。 -->
<div class="mx-auto flex max-w-2xl flex-col px-5 pb-safe-8">
	<!-- ログインしていなければ、ホームからログイン画面へ送られる。 -->
	<PageHeader backHref={resolve('/')} title={m.help_title()} />

	<div class="flex flex-col gap-4">
		{#each sections as section, i (section)}
			<section aria-labelledby="help-section-{i}">
				<h2 id="help-section-{i}" class={SECTION_TITLE_CLASS}>{section.title()}</h2>
				<div class="divide-y overflow-hidden rounded-lg raised bg-card">
					{#each section.topics as topic (topic.slug)}
						<a href={resolve('/help/[slug]', { slug: topic.slug })} class={LIST_ROW_CLASS}>
							{topic.title()}
							<ChevronRightIcon
								size={16}
								strokeWidth={2.2}
								class="text-subtle-foreground"
								aria-hidden="true"
							/>
						</a>
					{/each}
				</div>
			</section>
		{/each}
	</div>
</div>
