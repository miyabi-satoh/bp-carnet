<script lang="ts">
	import type { Snippet } from 'svelte';
	import { prefersReducedMotion } from 'svelte/motion';
	import { buildChartData, type ChartSeriesKey } from '$lib/bp-chart';
	import { delayedFlag, LOADING_SHOW_DELAY_MS } from '$lib/delayed-flag.svelte';
	import * as m from '$lib/paraglide/messages.js';
	import type { BpRecord, BpSummary } from '$lib/records';
	import BpChart from '$lib/components/bp-chart.svelte';
	import BpStatsSummary from '$lib/components/bp-stats-summary.svelte';
	import LoadError from '$lib/components/load-error.svelte';
	import LoadingIndicator from '$lib/components/loading-indicator.svelte';
	import RecordCard from '$lib/components/record-card.svelte';
	import { Button } from '$lib/components/ui/button';
	import { RECORD_LIST_PAGE_SIZE } from '$lib/records-view.svelte';

	/** 期間内の記録の表示。並びはデザインどおり、平均 → グラフ → 記録一覧。メイン画面と、
	 * 管理者による利用者の閲覧ページで共有する。取得と期間の切り替えは呼び出し側で行う。
	 *
	 * 縦に並べる間隔は、置き場所の `flex flex-col gap-*` に任せる。 */
	let {
		records,
		summary,
		loading,
		errorMessage,
		onretry,
		visibleKeys = $bindable(),
		listLimit = $bindable(),
		onedit,
		ondelete,
		timeZone,
		header,
		emptyMessage = m.record_empty()
	}: {
		records: BpRecord[];
		summary: BpSummary | null;
		loading: boolean;
		/** 取得に失敗したときの文言。空文字なら失敗していない。 */
		errorMessage: string;
		/** 取得をやり直す。失敗の表示に「もう一度試す」を出す。 */
		onretry: () => Promise<unknown>;
		/** グラフに描く系列。凡例のボタンで書き換わる。 */
		visibleKeys: ChartSeriesKey[];
		/** 記録一覧に出す件数 (新しい順)。「さらに表示」で増える。平均・グラフは全件で出す。 */
		listLimit: number;
		/** 渡さなければ記録カードに直す・消すボタンを出さない (読み取り専用)。 */
		onedit?: (record: BpRecord) => void;
		ondelete?: (record: BpRecord) => void;
		/** 記録カードで今年かどうかを決めるタイムゾーン。省略時はログイン中ユーザーの個人設定。 */
		timeZone?: string;
		/** 記録一覧の上に置く見出しの行。 */
		header?: Snippet;
		/** 期間内に記録が無いときの文。既定は「この期間の記録はありません。」。 */
		emptyMessage?: string;
	} = $props();

	let chartData = $derived(buildChartData(summary?.days ?? []));
	let shownRecords = $derived(records.slice(0, listLimit));
	let moreCount = $derived(Math.min(records.length - listLimit, RECORD_LIST_PAGE_SIZE));

	/** 前に出しておける内容があるか。取得に成功していれば、記録が0件でも集計は返る。
	 * 無いとき (初回・初回の取得に失敗した後) は、読み込み中を遅らせずに出す。遅らせると、
	 * 取得し直している間「この期間の記録はありません」と誤って伝えてしまうため。 */
	let hasContent = $derived(summary !== null);

	/** 期間を切り替えたときの読み込み中の表示。速い応答では出さず、前の内容を出したままにする。 */
	const showLoading = delayedFlag(() => loading, LOADING_SHOW_DELAY_MS);
</script>

{#if loading && (!hasContent || showLoading.current)}
	<LoadingIndicator />
{:else if errorMessage}
	<LoadError message={errorMessage} {onretry} />
{:else}
	{#if records.length > 0}
		{#if chartData.length === 0}
			<!-- 記録はあるが、どれも朝/夜の時間帯に入らない場合。履歴には出るのに
			     平均もグラフも空になる理由を伝える。 -->
			<p class="text-sm text-muted-foreground">{m.bp_summary_empty()}</p>
		{:else}
			<BpStatsSummary morning={summary?.morning} evening={summary?.evening} />
		{/if}
		{#if chartData.length >= 2}
			<!-- 1件だけでは折れ線が引けず空の枠だけが残るため、2件以上のときだけ表示する。 -->
			<div class="rounded-lg raised bg-card p-4">
				<BpChart
					data={chartData}
					bind:visibleKeys
					animate={!prefersReducedMotion.current}
					containerClass="h-52 sm:h-64"
				/>
			</div>
		{/if}
	{/if}
	<section class="flex flex-col gap-2.5">
		{@render header?.()}
		{#if records.length === 0}
			<p class="text-sm text-muted-foreground">{emptyMessage}</p>
		{:else}
			<div class="flex flex-col gap-2">
				{#each shownRecords as record (record.id)}
					<RecordCard {record} {timeZone} {onedit} {ondelete} />
				{/each}
			</div>
			{#if moreCount > 0}
				<Button variant="outline" onclick={() => (listLimit += RECORD_LIST_PAGE_SIZE)}>
					{m.record_list_show_more_button({ count: moreCount })}
				</Button>
			{/if}
		{/if}
	</section>
{/if}
