<script lang="ts">
	import type { Component } from 'svelte';
	import { setHelpTopicSlug, type HelpTopic } from '$lib/help';

	/** テスト用: 項目ページと同じく slug を渡して、本文を描く。 */
	let { topic, Content }: { topic: HelpTopic; Content: Component } = $props();

	setHelpTopicSlug(() => topic.slug);
</script>

<Content />
