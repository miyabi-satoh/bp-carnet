<script lang="ts">
	import { scaleTime } from 'd3-scale';
	import { curveMonotoneX } from 'd3-shape';
	import { LineChart, Points, Spline } from 'layerchart';
	import {
		buildChartConfig,
		buildChartSeries,
		buildChartYDomain,
		latestPointOf,
		visibleChartSeries,
		type ChartPoint,
		type ChartSeriesKey
	} from '$lib/bp-chart';
	import { formatDateTime, formatMonthDay } from '$lib/period';
	import * as Chart from '$lib/components/ui/chart';
	import BpChartLegend from '$lib/components/bp-chart-legend.svelte';
	import * as m from '$lib/paraglide/messages.js';

	type ChartSize = { width: number; height: number };

	/** X軸の左右の余白 (px)。無いと最初の日の線がY軸に貼り付き、最新の点の円が枠の端で欠ける。 */
	const X_PADDING = [16, 16];

	/** 朝/夜の血圧推移グラフ。メイン画面と印刷レポートで共有する。
	 *
	 * 描く系列・アニメーションの有無・コンテナの高さ・紙面に出す大きさだけが画面ごとに異なり、
	 * 軸・ツールチップ・凡例の作りは共通なので、差分を props に切り出してまとめている。 */
	let {
		data,
		visibleKeys = $bindable(),
		animate = false,
		containerClass = '',
		printSize
	}: {
		data: ChartPoint[];
		/** 描く系列。凡例のボタンで書き換わる (`bind:visibleKeys`)。 */
		visibleKeys: ChartSeriesKey[];
		/** グラフ描画時のアニメーション。印刷レポートは常に無効 (アニメーション途中の状態が
		 * 紙に出るのを避けるため)。メイン画面は「視差効果を減らす」設定に従う。 */
		animate?: boolean;
		containerClass?: string;
		/** 指定すると、紙面にはこの大きさで描いた別のグラフを出す。LayerChart は大きさを
		 * ResizeObserver で測るため、印刷時のレイアウト変更に追従せず、画面に描いた大きさのまま
		 * 紙に出てしまう (はみ出す・空になる)。 */
		printSize?: ChartSize;
	} = $props();

	const allSeries = buildChartSeries();
	let series = $derived(visibleChartSeries(visibleKeys));
	let config = $derived(buildChartConfig(series));
	let yDomain = $derived(buildChartYDomain(data, series));
</script>

<!-- `size` を渡すと大きさを測らずにその大きさで描く (紙面用)。 -->
{#snippet lineChart(size?: ChartSize)}
	<LineChart
		{data}
		x="date"
		xScale={scaleTime()}
		xPadding={X_PADDING}
		{yDomain}
		{series}
		width={size?.width}
		height={size?.height}
		props={{
			xAxis: {
				format: formatMonthDay,
				ticks: data.map((point) => point.date),
				// 月表示のように日数が多いと日ごとのラベルが重なるため、重なるものを間引く。期間の
				// 始まりと最新の日は残す。
				tickOcclusion: { priority: 'start-end', padding: 8 },
				// 間引きはこの文字サイズでラベル幅を測る。`Chart.Container` が CSS で text-xs (12px) に
				// しているのと揃えないと、実際より狭く見積もって重なりが残る。
				tickLabelProps: { fontSize: 12 }
			},
			highlight: { points: { r: 4 } }
		}}
	>
		<!-- 既定の marks は系列ごとに Spline を描くだけなので、最新点の円マークを足すために
		     差し替える。1日分しか値が無い系列は Spline が長さ0のパス (`M…Z`) になり、
		     stroke-linecap では点として描かれないため、円が無いと画面上に何も出ない。
		     `context.series.visibleSeries` ではなく自前の `series` を回す: 表示の切り替えは
		     `series` 自体を絞る形で行い、layerchart 内部の表示状態は使わないので差が無く、
		     内部APIに寄りかからずに済む。 -->
		{#snippet marks()}
			{#each series as s (s.key)}
				<Spline
					seriesKey={s.key}
					curve={curveMonotoneX}
					motion={animate && !size ? 'tween' : 'none'}
					strokeWidth={2}
				/>
				{@const latest = latestPointOf(data, s.key)}
				{#if latest}
					<Points seriesKey={s.key} data={[latest]} r={4.5} fill={s.color} />
				{/if}
			{/each}
		{/snippet}
		{#snippet tooltip()}
			<Chart.Tooltip labelFormatter={formatDateTime} />
		{/snippet}
	</LineChart>
{/snippet}

<BpChartLegend series={allSeries} bind:visibleKeys />
<!-- `aspect-auto w-full`: 既定の `aspect-video` のまま高さを `max-h-*` で抑えると、縦横比を保つために
     幅まで縮んでグラフが枠の左に寄る。高さは `containerClass` で決める。 -->
<!-- 読み上げでは線の中身を追えないので、1枚の絵として扱い、値の読める場所を案内する (WCAG 1.1.1)。 -->
<Chart.Container
	{config}
	role="img"
	aria-label={m.bp_chart_label()}
	class={['aspect-auto w-full', containerClass, printSize && 'print:hidden']}
>
	{@render lineChart()}
</Chart.Container>
{#if printSize}
	<Chart.Container {config} class="hidden aspect-auto print:flex">
		{@render lineChart(printSize)}
	</Chart.Container>
{/if}
