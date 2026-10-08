<script lang="ts">
	import type { Snippet } from 'svelte';
	import * as m from '$lib/paraglide/messages.js';

	/** 記録の中身の組み方 (日時 → 上/下・脈拍 → メモ)。ホームの記録カードと
	 * 取り込みの行で共通。`<label>` の中にも置けるよう、要素は `span` で組む。 */
	let {
		dateTime,
		systolic,
		diastolic,
		pulse,
		memo,
		pulseAsValue = false,
		badge
	}: {
		/** 表示用に整えた日時。 */
		dateTime: string;
		systolic: number | string;
		diastolic: number | string;
		/** 無い (`null`・`undefined`・空文字) なら出さない。 */
		pulse: number | string | null | undefined;
		memo: string;
		/** 脈拍の値を上/下と同じ大きさ・太さで見せるか (取り込みの一覧。読み取った値として確かめる対象のため、docs/import-export.md)。 */
		pulseAsValue?: boolean;
		/** 日時の後ろに添える印 (取り込みの一覧の「要確認」など)。 */
		badge?: Snippet;
	} = $props();
</script>

<span class="flex flex-wrap items-center gap-x-2 gap-y-1 text-sm text-muted-foreground">
	{dateTime}
	{@render badge?.()}
</span>
<span class="mt-0.5 block text-lg font-bold">
	{systolic}<span class="px-1 text-sm font-medium text-muted-foreground">/</span>{diastolic}
	{#if pulse !== null && pulse !== undefined && pulse !== ''}
		{#if pulseAsValue}
			<span class="ml-2"
				><span class="pr-1 text-sm font-medium text-muted-foreground">{m.record_pulse_label()}</span
				>{pulse}</span
			>
		{:else}
			<span class="ml-1 text-xs font-medium text-subtle-foreground">
				{m.record_pulse_with_value({ pulse })}
			</span>
		{/if}
	{/if}
</span>
{#if memo}
	<span class="mt-1 block text-sm break-words whitespace-pre-line text-subtle-foreground"
		>{memo}</span
	>
{/if}
