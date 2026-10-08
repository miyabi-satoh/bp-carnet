<script lang="ts">
	import ChevronRightIcon from '@lucide/svelte/icons/chevron-right';
	import CircleAlertIcon from '@lucide/svelte/icons/circle-alert';
	import { resolve } from '$app/paths';
	import OcrQuotaGauge from '$lib/components/ocr-quota-gauge.svelte';
	import type { OcrQuota } from '$lib/ocr';
	import * as m from '$lib/paraglide/messages.js';

	/** 写真で記録の、読み取りの枠の残り (docs/ocr.md)。今使っている枠を1本だけ、見出し・ゲージ・割合の
	 * 同じ枠で出し、量に応じて色と文言だけを変える。使い切りはデザインの残量バナー。
	 * - `quota` は今使っている枠 (使う順で最初の、残りのある枠)。使い切り (`exhausted`) なら使わない。
	 * - `low` は全部の枠を足した残りが少ないとき。ゲージと割合を注意の色にする。
	 * - `othersRemaining` はほかにも残りのある枠があるとき。設定の枠の一覧へ案内する。
	 * - 使い切りは、買い足せるなら買い足しを、そうでなければ数字での記録を次の一手として添える。
	 *   支援技術へ通知されるようライブリージョンにし、読み取りの途中で使い切ったとき (`urgent`、429) は
	 *   `role="alert"`、開いた時点で使い切っているときは控えめな `role="status"`。 */
	let {
		quota,
		exhausted,
		low,
		othersRemaining,
		showTopup,
		urgent
	}: {
		quota: OcrQuota | null;
		exhausted: boolean;
		low: boolean;
		othersRemaining: boolean;
		showTopup: boolean;
		urgent: boolean;
	} = $props();
</script>

<div
	role={exhausted ? (urgent ? 'alert' : 'status') : undefined}
	class={[
		'flex flex-col gap-2 rounded-xl px-4 py-3',
		exhausted ? 'border-2 border-warn-border bg-warn-soft' : 'border border-border bg-card'
	]}
>
	{#if exhausted || !quota}
		<div class="flex items-center gap-2.5">
			<span
				class="flex size-8 shrink-0 items-center justify-center rounded-full bg-card text-warn-foreground"
				aria-hidden="true"
			>
				<CircleAlertIcon size={18} strokeWidth={2.2} />
			</span>
			<p class="text-base font-bold text-warn-foreground">
				{m.photo_page_quota_exhausted_title()}
			</p>
		</div>
		<p class="text-sm leading-relaxed text-muted-foreground">
			{showTopup ? m.photo_page_quota_exhausted_topup() : m.photo_page_quota_exhausted_manual()}
		</p>
		<OcrQuotaGauge percent={0} exhausted />
	{:else}
		<p class="text-sm font-bold">
			{quota.kind === 'paid' ? m.ocr_quota_paid_title() : m.ocr_quota_free_title()}
		</p>
		<OcrQuotaGauge percent={quota.remainingPercent} {low} />
		{#if othersRemaining}
			<!-- 設定の「写真の読み取り」の節へ移る。resolve() はハッシュを付けられないので、付けた後の値を渡す。 -->
			<!-- eslint-disable svelte/no-navigation-without-resolve -->
			<a
				href={resolve('/settings') + '#ocr'}
				class="flex items-center gap-0.5 self-start text-sm font-bold text-primary"
			>
				{m.photo_page_other_quotas_link()}
				<ChevronRightIcon size={16} strokeWidth={2.2} aria-hidden="true" />
			</a>
			<!-- eslint-enable svelte/no-navigation-without-resolve -->
		{/if}
	{/if}
</div>
