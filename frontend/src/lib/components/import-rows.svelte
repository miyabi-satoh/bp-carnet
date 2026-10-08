<script lang="ts" generics="T extends ImportRow">
	import type { Snippet } from 'svelte';
	import type { ImportRow, ImportRowStatus } from '$lib/import';
	import * as m from '$lib/paraglide/messages.js';
	import { formatImportRowDateTime } from '$lib/period';
	import { dayPeriodAt, type PeriodThresholds } from '$lib/settings';
	import { Button } from '$lib/components/ui/button';
	import { Checkbox } from '$lib/components/ui/checkbox';
	import NoticeBox from '$lib/components/notice-box.svelte';
	import RecordValues from '$lib/components/record-values.svelte';
	import CheckIcon from '@lucide/svelte/icons/check';
	import CircleAlertIcon from '@lucide/svelte/icons/circle-alert';
	import ListFilterIcon from '@lucide/svelte/icons/list-filter';
	import PencilIcon from '@lucide/svelte/icons/pencil';

	/** 読み込んだ行の一覧 (docs/import-export.md)。CSV でも手書きメモの読み取りでも同じ形で並べ、
	 * 値を直すのは行の「直す」から開くシートに任せる (ラベルの無い欄に必須の印を置けないため、
	 * 一覧の中では直さない)。 */
	let {
		rows = $bindable(),
		periods,
		thisYear,
		statusOf,
		noteOf,
		disabled = false,
		noneKeptNote,
		beforeList,
		onedit,
		onkeepchange
	}: {
		rows: T[];
		/** 朝/夜の時間帯。行の日時に「朝」「夜」を添えるのに使う。 */
		periods: PeriodThresholds;
		/** 今年 (`YYYY`、`currentYearIn`)。今年でない行にだけ年を付ける。 */
		thisYear: string;
		/** 行の状態。読み取りの自信を見るかどうかが CSV と写真で違うので、呼び出し側が決める。 */
		statusOf: (row: T) => ImportRowStatus;
		/** 行に添える「何をすればよいか」の文。無ければ出さない。 */
		noteOf: (row: T) => string | undefined;
		/** 取り込みの送信中など、操作を受け付けないとき。 */
		disabled?: boolean;
		/** 取り込める記録が1件も無いときのお知らせに足す一文。写真では読み取り直しの案内を渡す。 */
		noneKeptNote?: string;
		/** 見出し・説明文と一覧の間に置くもの。写真では、行に書かれていない年を選ぶ欄。 */
		beforeList?: Snippet;
		/** 行の「直す」が押されたとき。 */
		onedit: (row: T) => void;
		/** チェックが変わったとき。 */
		onkeepchange?: () => void;
	} = $props();

	const headingId = $props.id();

	/** 確かめてほしい行だけに絞るか。行が多いときのための絞り込み (docs/import-export.md)。 */
	let needsReviewOnly = $state(false);

	// 送る行の条件 (`$lib/import` の `includedRows`) と揃える。チェックの入ったエラー行を
	// 数えると、出した件数より送る件数が少なくなる。
	const keptCount = $derived(rows.filter((row) => row.keep && !row.rowError).length);
	const shown = $derived(
		rows
			.map((row, index) => ({ row, index, status: statusOf(row) }))
			.filter((item) => !needsReviewOnly || item.status !== 'ok' || item.row.fixed)
	);
	// 直した記録も絞り込みに残す (直した結果をその場で見返せるように)。
	const needsReviewCount = $derived(
		rows.filter((row) => statusOf(row) !== 'ok' || row.fixed).length
	);

	// 最後の1件を直すと絞り込みのボタンごと消えるので、絞り込みも解く。残したままだと、
	// 空の一覧に取り残されて解く手段が画面から無くなる。
	$effect(() => {
		if (needsReviewCount === 0) needsReviewOnly = false;
	});

	function label(row: ImportRow): string {
		// 日付も時刻も読めなかった行は、空のままだと直すボタンの名前が崩れるので「日時なし」と出す。
		return (
			formatImportRowDateTime(
				row.measuredOnDate,
				row.time,
				dayPeriodAt(row.time, periods),
				row.timeIsPlaceholder ?? false,
				thisYear
			) || m.import_rows_no_datetime()
		);
	}
</script>

<section class="flex flex-col gap-3" aria-labelledby={headingId}>
	<div class="flex flex-col gap-1">
		<h2 id={headingId} class="text-base font-extrabold">{m.import_rows_title()}</h2>
		<p class="text-sm leading-relaxed text-muted-foreground">{m.import_rows_description()}</p>
	</div>
	{@render beforeList?.()}
	<!-- 取り込める記録が1件も無い間は、理由と次の操作を一覧の上に出す (docs/ocr.md)。
	     CSV でも写真でも同じ見せ方にし、写真だけ読み取り直しの案内を足す。
	     日本語は句点で切れるのでそのまま繋ぐ。言語を足すときは、足す側の文言の先頭に区切りを入れる。 -->
	{#if keptCount === 0}
		<NoticeBox variant="muted" message={m.import_rows_description_none() + (noneKeptNote ?? '')} />
	{/if}
	<div class="flex flex-wrap items-center justify-between gap-2">
		<p class="text-sm text-muted-foreground">
			{m.import_rows_count({ total: rows.length, kept: keptCount })}
		</p>
		{#if needsReviewCount > 0}
			<!-- 押せるものは押せるように見せる ため、絞り込みは枠のあるボタンにする。 -->
			<Button
				type="button"
				variant={needsReviewOnly ? 'default' : 'outline'}
				size="sm"
				aria-pressed={needsReviewOnly}
				{disabled}
				onclick={() => (needsReviewOnly = !needsReviewOnly)}
			>
				<ListFilterIcon />
				{m.import_rows_needs_review_filter()}
			</Button>
		{/if}
	</div>

	{#if shown.length === 0}
		<p class="text-sm text-muted-foreground">{m.import_rows_needs_review_empty()}</p>
	{/if}

	<ul class="flex flex-col gap-3">
		{#each shown as { row, index, status } (index)}
			{@const note = noteOf(row)}
			<li
				class={[
					'flex items-start justify-between gap-3 rounded-md raised bg-card p-3 sm:p-4',
					// 取り込まない記録は薄くする。「要確認」の記録は薄くしない (docs/import-export.md)。
					!row.keep && status === 'ok' && 'opacity-60',
					// 枠の色は「要確認」のバッジの補助 (色だけで見分けさせない、docs/import-export.md)。
					status === 'error' && 'ring-2 ring-destructive/70',
					status === 'warn' && 'ring-2 ring-coral-foreground/70'
				]}
			>
				<!-- 直さないと取り込めない記録はチェックできない (docs/import-export.md)。
				     直すと書き戻しでチェックが入る (`applyEditedRow`)。 -->
				<Checkbox
					id="import-row-{index}"
					class="mt-1"
					disabled={disabled || status === 'error'}
					bind:checked={
						() => row.keep && status !== 'error',
						(v) => {
							row.keep = v;
							onkeepchange?.();
						}
					}
					aria-label={m.record_row_keep_label()}
				/>
				<!-- ホームの記録カードと同じ組み方 (日時 → 上/下・脈拍 → メモ)。カードと同じく文字は押せない
				     (値を確かめようと触れただけでチェックが外れないように。直すのは鉛筆のボタン)。 -->
				<div class="min-w-0 flex-1">
					<RecordValues
						dateTime={label(row)}
						systolic={row.systolic}
						diastolic={row.diastolic}
						pulse={row.pulse}
						pulseAsValue
						memo={row.memo}
					>
						{#snippet badge()}
							{#if status !== 'ok'}
								<span
									class={[
										'rounded-full border px-2 py-0.5 text-xs font-bold',
										status === 'error'
											? 'border-destructive/70 text-destructive'
											: 'border-coral-foreground/70 text-coral-foreground'
									]}
								>
									{m.import_rows_needs_review_badge()}
								</span>
							{:else if row.fixed}
								<span
									class="inline-flex items-center gap-1 rounded-full border border-primary/70 px-2 py-0.5 text-xs font-bold text-primary"
								>
									<CheckIcon size={12} aria-hidden="true" />
									{m.import_rows_fixed_badge()}
								</span>
							{/if}
						{/snippet}
					</RecordValues>
					{#if note}
						<span
							class={[
								'mt-1 flex items-start gap-1.5 text-xs',
								status === 'error' ? 'text-destructive' : 'text-coral-foreground'
							]}
						>
							<CircleAlertIcon size={14} class="mt-px shrink-0" aria-hidden="true" />
							{note}
						</span>
					{/if}
				</div>
				<Button
					type="button"
					variant="ghost"
					size="icon"
					class="-mt-1.5 shrink-0 rounded-full"
					{disabled}
					aria-label={m.import_rows_edit_label({ measuredAt: label(row) })}
					onclick={() => onedit(row)}
				>
					<PencilIcon />
				</Button>
			</li>
		{/each}
	</ul>
</section>
