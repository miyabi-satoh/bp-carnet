<script lang="ts">
	import { backOrLogin } from '$lib/back-or-login';
	import PageHeader from '$lib/components/page-header.svelte';
	import { isNativeApp } from '$lib/native-app';
	import * as m from '$lib/paraglide/messages.js';
	import { SECTION_TITLE_CLASS } from '$lib/styles';
	import ChevronRightIcon from '@lucide/svelte/icons/chevron-right';
	import { SvelteSet } from 'svelte/reactivity';
	import type { DisplayPackage, LicenseDisplay } from '../../../scripts/license-display';
	import type { PageProps } from './$types';

	let { data }: PageProps = $props();

	// iPhone アプリの部品は、アプリで開いたときだけ出す。ブラウザには配っていないため。
	let sections = $derived([
		{ key: 'web', title: m.licenses_section_web(), list: data.web },
		...(isNativeApp ? [{ key: 'ios', title: m.licenses_section_ios(), list: data.ios }] : [])
	]);

	/** 開いている行。本文は長いので、開いた行の分だけ描く。節ごとに同じ名前がありうるので、節の名前を添える。 */
	const opened = new SvelteSet<string>();

	function onToggle(event: Event, key: string) {
		if ((event.currentTarget as HTMLDetailsElement).open) opened.add(key);
		else opened.delete(key);
	}

	function textsOf(list: LicenseDisplay, pkg: DisplayPackage) {
		return pkg.texts.map((index) => list.texts[index]);
	}

	const back = backOrLogin();
</script>

<div class="mx-auto flex max-w-2xl flex-col px-5 pb-safe-8">
	<PageHeader onback={back} title={m.licenses_title()} />

	<p class="text-sm leading-7 text-muted-foreground">{m.licenses_lead()}</p>

	{#each sections as section (section.key)}
		<section class="mt-6" aria-labelledby="licenses-{section.key}">
			<h2 id="licenses-{section.key}" class={SECTION_TITLE_CLASS}>{section.title}</h2>
			{#if section.list === null}
				<p class="text-sm text-muted-foreground">{m.licenses_unavailable()}</p>
			{:else}
				{@const list = section.list}
				<ul class="divide-y overflow-hidden rounded-lg raised bg-card">
					{#each list.packages as pkg (pkg.name)}
						{@const key = `${section.key}:${pkg.name}`}
						<li>
							<details class="group" ontoggle={(event) => onToggle(event, key)}>
								<!-- 開閉の印は自前で置く。summary を flex にすると既定の三角が消えるため。 -->
								<summary
									class="flex min-h-13 cursor-pointer items-center gap-2 px-4 py-3 text-sm select-none hover:bg-muted"
								>
									<ChevronRightIcon
										size={16}
										strokeWidth={2.2}
										class="shrink-0 text-subtle-foreground transition-transform group-open:rotate-90"
										aria-hidden="true"
									/>
									<!-- スマホの幅では、式をいつも次の行に左寄せで置く。入りきらない行だけ回すと、
									     行ごとに式の位置が変わって揃わない。広い幅では同じ行の右に寄せる。 -->
									<span class="flex min-w-0 flex-1 flex-wrap items-baseline gap-x-2 gap-y-0.5">
										<span class="font-bold wrap-anywhere">{pkg.name}</span>
										<span class="text-muted-foreground">{pkg.versions.join(', ')}</span>
										<!-- 式は語の区切りでだけ折る。`Apache-2.0` がハイフンで折れると読めない。 -->
										<span
											class="basis-full text-muted-foreground sm:ml-auto sm:basis-auto sm:text-right"
											>{#each pkg.license.split(' ') as term, i (i)}{i > 0 ? ' ' : ''}<span
													class="whitespace-nowrap">{term}</span
												>{/each}</span
										>
									</span>
								</summary>
								{#if opened.has(key)}
									<div class="space-y-3 px-4 pb-4">
										{#if pkg.repository !== null}
											<!-- MPL-2.0 の部品があり、受け取る人にソースの入手先を知らせる必要があるので、
											     置き場所を出す。リンクは開閉の summary の外に置く。中に置くと、押したときに
											     開閉とリンクのどちらが働くかが紛らわしい。 -->
											<p class="flex flex-wrap items-center gap-x-1 text-sm">
												{m.licenses_source()}:
												<a
													href={pkg.repository}
													target="_blank"
													rel="external noopener noreferrer"
													class="inline-flex min-h-11 items-center wrap-anywhere text-accent-foreground underline underline-offset-4"
												>
													{pkg.repository}<span class="sr-only">{m.common_opens_in_new_tab()}</span>
												</a>
											</p>
										{/if}
										{#each textsOf(list, pkg) as text, index (index)}
											<div>
												<h3 class="text-xs font-bold text-muted-foreground">{text.name}</h3>
												<!-- URL・罫線など、空白の無い長い並びが狭い幅で枠を越えるので、どこでも折り返す。 -->
												<p
													class="mt-1 rounded-md bg-muted px-3 py-2 text-xs leading-relaxed wrap-anywhere whitespace-pre-wrap"
												>
													{text.text}
												</p>
											</div>
										{/each}
									</div>
								{/if}
							</details>
						</li>
					{/each}
				</ul>
			{/if}
		</section>
	{/each}
</div>
