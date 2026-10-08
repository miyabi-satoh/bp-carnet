<script lang="ts">
	import { goto } from '$app/navigation';
	import { page } from '$app/state';
	import { resolve } from '$app/paths';
	import { buildChartData, parseSeriesParam, type ChartSeriesKey } from '$lib/bp-chart';
	import { GENERIC_ERROR_MESSAGE } from '$lib/api/errors';
	import * as m from '$lib/paraglide/messages.js';
	import {
		formatDateLabel,
		formatLocalDateTimeWithWeekday,
		parseDateParam,
		parseLocalDate,
		formatOpenPeriodLabel
	} from '$lib/period';
	import { fetchRecordsWithSummary, type BpSummary, type RecordResponse } from '$lib/records';
	import { buildReportHref } from '$lib/report';
	import { printPage } from '$lib/native-app';
	import PrinterIcon from '@lucide/svelte/icons/printer';
	import { Button } from '$lib/components/ui/button';
	import PageHeader from '$lib/components/page-header.svelte';
	import BpStatsSummary from '$lib/components/bp-stats-summary.svelte';
	import BpChart from '$lib/components/bp-chart.svelte';
	import LoadingIndicator from '$lib/components/loading-indicator.svelte';
	import LoadError from '$lib/components/load-error.svelte';
	import NoticeDialog from '$lib/components/notice-dialog.svelte';
	import PeriodNav from '$lib/components/period-nav.svelte';

	// 期間 (`from`/`to`、YYYY-MM-DD) と描く系列 (`series`) はURLクエリだけから決める。メイン画面から
	// 引き継いだ値も、ページ内で変えた値 (`updateQuery`) もここを通る。`from`/`to` がどちらも
	// 無ければ全件が対象。日付として読めない値は指定なしとして扱う。
	let fromParam = $derived(parseDateParam(page.url.searchParams.get('from')));
	let toParam = $derived(parseDateParam(page.url.searchParams.get('to')));
	let visibleSeriesKeys = $derived(parseSeriesParam(page.url.searchParams.get('series')));

	/** ページ内で変えた期間・系列をURLクエリに反映する。取得し直しは `fromParam`/`toParam` を見る
	 * `$effect` が行う。履歴を積まないのは、ブラウザの戻るで変更の途中経過をたどらずに
	 * メイン画面へ戻れるようにするため。 */
	function updateQuery(changes: { from: string; to: string } | { series: ChartSeriesKey[] }) {
		const next = { from: fromParam, to: toParam, series: visibleSeriesKeys, ...changes };
		// `buildReportHref` の中で resolve() 済み。クエリ付きのURLは resolve() の戻り値の型に
		// ならないため、この規則では追えない。
		// eslint-disable-next-line svelte/no-navigation-without-resolve
		goto(buildReportHref(next.from, next.to, next.series), {
			replaceState: true,
			noScroll: true,
			keepFocus: true
		});
	}

	/** 見出しに表示する対象期間ラベル。 */
	let periodLabel = $derived(
		formatOpenPeriodLabel(fromParam, toParam) ??
			m.period_range_label({
				from: formatDateLabel(parseLocalDate(fromParam)),
				to: formatDateLabel(parseLocalDate(toParam))
			})
	);

	let records = $state<RecordResponse[]>([]);
	let bpSummary = $state<BpSummary | null>(null);
	let loading = $state(true);
	let loadErrorMessage = $state('');
	let printErrorDialog = $state<ReturnType<typeof NoticeDialog> | null>(null);

	/** 印刷の画面を開く。アプリの印刷は失敗を返すことがあるので、エラーのダイアログに出す。 */
	async function openPrint() {
		try {
			await printPage();
		} catch {
			printErrorDialog?.show(m.report_print_error_title(), GENERIC_ERROR_MESSAGE());
		}
	}

	let chartData = $derived(buildChartData(bpSummary?.days ?? []));

	/** 表のセルの余白。紙面では少し広げる。 */
	const cellClass = 'px-1.5 py-2 print:px-2.5 print:py-1.5';
	/** スマホ幅では5列が収まらないため、上/下の血圧を1列にまとめる。
	 * 紙面は開いた端末によらず5列にする。 */
	const wideOnlyClass = 'hidden sm:table-cell print:table-cell';
	const narrowOnlyClass = 'sm:hidden print:hidden';

	/** 取得結果を state に反映してよいかどうかを `isCancelled()` で確認しながら進める。
	 * `fromParam`/`toParam` が短時間に連続で変わると複数の取得が並行し得るため、後から
	 * 発行されたリクエストが先に完了して表示を更新した後に、古いリクエストの応答が
	 * 遅れて届いて上書きしてしまうのを防ぐ。 */
	async function loadRecords(from: string, to: string, isCancelled: () => boolean) {
		loading = true;
		loadErrorMessage = '';
		const result = await fetchRecordsWithSummary(from, to);
		if (isCancelled()) return;
		loading = false;
		if (!result.ok) {
			loadErrorMessage = result.message;
			return;
		}
		// 一覧はメイン画面の履歴と同じ新しい順のまま印刷する。
		records = result.records;
		bpSummary = result.summary;
	}

	/** 失敗した取得をやり直す。やり直しの間に期間が変わったら、その結果は捨てる。 */
	function retryLoad() {
		const from = fromParam;
		const to = toParam;
		return loadRecords(from, to, () => from !== fromParam || to !== toParam);
	}

	/** 紙面 (A4横) に出すグラフの大きさ (`BpChart` の `printSize`)。幅は用紙の297mmから
	 * `@page` の余白・ページの p-6・平均カードの列 (w-70 と gap-12)・グラフのカードの p-3 と
	 * 枠線を引いた値なので、これらを変えたら合わせて直す。 */
	const printChartSize = { width: 630, height: 192 };

	// 初回表示、および `fromParam`/`toParam` (URLクエリ) が変わるたびにデータを取得し直す。
	// 短時間に続けて変わると前回の取得がまだ進行中のことがあるため、cleanup で cancelled を
	// 立てて古い方の取得を打ち切る (loadRecords 参照)。
	// 印刷の画面は自動では開かない。「印刷する」ボタンで開く。
	$effect(() => {
		const from = fromParam;
		const to = toParam;
		let cancelled = false;
		loadRecords(from, to, () => cancelled);
		return () => {
			cancelled = true;
		};
	});
</script>

<!-- 印刷時は p-6 を維持する: 0 にすると LineChart の Y軸ラベル (2桁→3桁の先頭桁) が
     コンテナ左端でクリップされる (@page の margin だけでは軸ラベル分の余白が足りない)。
     画面では、上の余白は PageHeader が持つ (上端の帯を含む)。 -->
<div class="mx-auto flex max-w-5xl flex-col gap-4 px-6 pb-safe-6 print:max-w-none print:p-6">
	<!-- 共通ヘッダーの代わりに、ほかの画面と同じ丸い戻るボタンと見出しを置く。紙には出さない。 -->
	<div class="print:hidden">
		<PageHeader backHref={resolve('/')} title={m.report_page_title()}>
			{#snippet actions()}
				<!-- 読み込み中に押すと「読み込み中...」や前の期間のまま紙に出るため、読み終えるまで押させない。 -->
				<Button type="button" disabled={loading} onclick={openPrint}>
					<PrinterIcon />
					{m.report_print_button()}
				</Button>
			{/snippet}
		</PageHeader>
	</div>

	<!-- 印刷する期間を選び直す。紙には出さない。 -->
	<div class="rounded-lg border bg-card p-4 print:hidden">
		<PeriodNav
			from={fromParam}
			to={toParam}
			{loading}
			onCard
			onchange={(from, to) => updateQuery({ from, to })}
		/>
	</div>

	<!-- 紙面はタイトルと期間を1行に並べる。スマホ幅では2行にする。 -->
	<div
		class="flex flex-col sm:flex-row sm:items-baseline sm:justify-between print:flex-row print:items-baseline print:justify-between"
	>
		<h2 class="text-lg font-bold sm:text-2xl print:text-2xl">{m.report_title()}</h2>
		<p class="text-sm text-muted-foreground">{periodLabel}</p>
	</div>

	{#if loading}
		<LoadingIndicator />
	{:else if loadErrorMessage}
		<LoadError message={loadErrorMessage} onretry={retryLoad} />
	{:else if records.length === 0}
		<p class="text-sm text-muted-foreground">{m.report_empty()}</p>
	{:else}
		{#if records.length >= 2}
			<!-- 紙面は平均カードの縦積み (左) とグラフ (右) の2カラム。画面でも広い幅では紙面と
		     同じ並びにする。1件だけならテーブルだけにする。 -->
			<div class="flex flex-col gap-4 lg:flex-row lg:gap-12 print:flex-row print:gap-12">
				{#if chartData.length === 0}
					<p class="text-sm text-muted-foreground">{m.bp_summary_empty()}</p>
				{:else}
					<BpStatsSummary
						morning={bpSummary?.morning}
						evening={bpSummary?.evening}
						unit="inline"
						class="lg:w-70 lg:shrink-0 lg:grid-cols-1 lg:content-start print:w-70 print:shrink-0 print:grid-cols-1 print:content-start"
					/>
				{/if}
				{#if chartData.length >= 2}
					<!-- 1件だけでは折れ線が引けず空の枠だけが残るため、2件以上のときだけ表示する。
				     幅は BpChart の既定 (w-full)、高さはここで決める。 -->
					<div
						class="min-w-0 flex-1 rounded-lg raised bg-card p-4 print:border print:p-3 print:shadow-none"
					>
						<BpChart
							data={chartData}
							bind:visibleKeys={() => visibleSeriesKeys, (series) => updateQuery({ series })}
							containerClass="h-64"
							printSize={printChartSize}
						/>
					</div>
				{/if}
			</div>
		{/if}
		<table class="w-full text-sm tabular-nums print:text-xs">
			<thead>
				<tr class="border-b-2 text-left font-bold whitespace-nowrap text-muted-foreground">
					<th class={cellClass}>{m.report_table_datetime_label()}</th>
					<th class={[cellClass, wideOnlyClass]}>{m.record_systolic_label()}</th>
					<th class={[cellClass, wideOnlyClass]}>{m.record_diastolic_label()}</th>
					<th class={[cellClass, narrowOnlyClass]}>{m.record_bp_pair_label()}</th>
					<th class={cellClass}>{m.record_pulse_label()}</th>
					<th class={cellClass}>{m.record_memo_label()}</th>
				</tr>
			</thead>
			<tbody>
				{#each records as record (record.id)}
					{@const measuredAt = formatLocalDateTimeWithWeekday(record.localMeasuredAt)}
					<tr class="border-b print:break-inside-avoid">
						<!-- スマホ幅では日付と時刻を2段にし、メモの幅を空ける。 -->
						<td class={[cellClass, 'whitespace-nowrap']}
							>{measuredAt.date}<span class="block sm:ml-1 sm:inline print:ml-1 print:inline"
								>{measuredAt.time}</span
							></td
						>
						<td class={[cellClass, wideOnlyClass]}>{record.systolic}</td>
						<td class={[cellClass, wideOnlyClass]}>{record.diastolic}</td>
						<td class={[cellClass, narrowOnlyClass, 'whitespace-nowrap']}
							>{record.systolic} / {record.diastolic}</td
						>
						<td class={cellClass}>{record.pulse ?? m.common_no_data()}</td>
						<td class={cellClass}>{record.memo}</td>
					</tr>
				{/each}
			</tbody>
		</table>
	{/if}
</div>

<NoticeDialog bind:this={printErrorDialog} />
