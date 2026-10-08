<script lang="ts">
	import { backOrLogin } from '$lib/back-or-login';
	import { legalDocumentComponent, type LegalDocument } from '$lib/legal';
	import PageHeader from '$lib/components/page-header.svelte';

	/** 利用規約・プライバシーポリシーのページ。規約を出さない構成では、ページの load が 404 にする。 */
	let { document, title }: { document: LegalDocument; title: string } = $props();

	const Body = $derived(legalDocumentComponent(document));

	const back = backOrLogin();
</script>

<div class="mx-auto flex max-w-2xl flex-col px-5 pb-safe-8">
	<PageHeader onback={back} {title} />

	<!-- 本文はアプリに同梱した Markdown から作る ($lib/legal)。 -->
	<article class="legal-document rounded-lg raised bg-card p-5 text-sm leading-7">
		<Body />
	</article>
</div>

<style>
	/* 本文の Markdown から作った要素は、このページの外では使わないので、ここで見た目を付ける。 */
	.legal-document :global(p) {
		margin-block: 0.5rem;
	}
	.legal-document :global(h2) {
		margin-block: 1.25rem 0.5rem;
		font-size: var(--text-base);
		font-weight: 800;
	}
	.legal-document :global(ul) {
		margin-block: 0.25rem;
		padding-left: 1.25rem;
		list-style: disc;
	}
	.legal-document :global(a) {
		color: var(--color-accent-foreground);
		overflow-wrap: anywhere;
	}
	/* 表は画面の幅に収まらないので、表だけを横にスクロールさせる。 */
	.legal-document :global(table) {
		display: block;
		overflow-x: auto;
		margin-block: 0.75rem;
		border-collapse: collapse;
		font-size: var(--text-xs);
		line-height: 1.6;
	}
	.legal-document :global(th),
	.legal-document :global(td) {
		min-width: 7rem;
		border: 1px solid var(--color-border);
		padding: 0.375rem 0.5rem;
		text-align: left;
		vertical-align: top;
	}
</style>
