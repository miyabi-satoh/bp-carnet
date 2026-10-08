<script lang="ts">
	import { toggleSeriesKey, type ChartSeries, type ChartSeriesKey } from '$lib/bp-chart';
	import * as m from '$lib/paraglide/messages.js';

	/** 線サンプル付きの凡例。layerchart 標準の凡例は
	 * 色の四角しか描けないため、線種も描ける凡例に差し替えている。
	 *
	 * 画面では各項目を系列の表示切り替えボタンにし、`visibleKeys` を書き換える。印刷時は
	 * 押せないボタンや描いていない系列を紙に出さないよう、描く系列だけを並べる。 */
	let {
		series,
		visibleKeys = $bindable()
	}: {
		/** 全系列。 */
		series: ChartSeries[];
		visibleKeys: ChartSeriesKey[];
	} = $props();

	let visibleSeries = $derived(series.filter((s) => visibleKeys.includes(s.key)));
</script>

{#snippet sample(s: ChartSeries)}
	<svg width="20" height="6" aria-hidden="true">
		<line
			x1="0"
			y1="3"
			x2="20"
			y2="3"
			stroke={s.color}
			stroke-width="3"
			stroke-dasharray={s.dashed ? '5 3' : undefined}
		/>
	</svg>
{/snippet}

<div
	role="group"
	aria-label={m.chart_series_toggle_group_label()}
	class="mb-2 flex flex-wrap items-center gap-2 text-xs print:hidden"
>
	{#each series as s (s.key)}
		{@const visible = visibleKeys.includes(s.key)}
		<button
			type="button"
			aria-pressed={visible}
			disabled={visible && visibleKeys.length === 1}
			class="flex min-h-11 items-center gap-1.5 rounded-full border px-3 transition-colors outline-none focus-visible:border-ring focus-visible:ring-3 focus-visible:ring-ring/50 {visible
				? 'bg-background text-foreground'
				: 'text-subtle-foreground [&>svg]:opacity-30'}"
			onclick={() => (visibleKeys = toggleSeriesKey(visibleKeys, s.key))}
		>
			{@render sample(s)}
			{s.label}
		</button>
	{/each}
</div>
<div
	class="mb-2 hidden flex-wrap items-center gap-x-4 gap-y-1 text-xs text-muted-foreground print:flex"
>
	{#each visibleSeries as s (s.key)}
		<div class="flex items-center gap-1.5">
			{@render sample(s)}
			{s.label}
		</div>
	{/each}
</div>
