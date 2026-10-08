<script lang="ts">
	import {
		buildImportDiff,
		deletionWarning,
		fetchExistingEntries,
		importDateRange,
		type ExistingEntry,
		type ImportDiffRow,
		type ImportDiffStatus,
		type ImportRow
	} from '$lib/import';
	import * as m from '$lib/paraglide/messages.js';
	import { formatDateWithWeekday, parseLocalDate } from '$lib/period';
	import { Button } from '$lib/components/ui/button';
	import LoadingButton from '$lib/components/loading-button.svelte';
	import LoadingIndicator from '$lib/components/loading-indicator.svelte';
	import LoadError from '$lib/components/load-error.svelte';
	import NoticeBox from '$lib/components/notice-box.svelte';
	import ScreenFooter from '$lib/components/screen-footer.svelte';

	/** 取り込み内容の確認 (CSVの取り込みのページ・写真で記録のページで共通)。見出しは呼び出し側の
	 * ページがこの前に置く。
	 * 取り込みの実行は呼び出し側 (`ImportConfirmation`) が持ち、この画面は確定を知らせるだけにする。 */
	let {
		rows,
		importing,
		importError = '',
		notice = '',
		reloadKey = 0,
		onback,
		onconfirm
	}: {
		/** 取り込む行。`rowError` のある行は対象外 (`$lib/import`)。 */
		rows: ImportRow[];
		/** 取り込み中か。この間は左のボタンも押せなくする。 */
		importing: boolean;
		/** 取り込みに失敗したときの文言。 */
		importError?: string;
		/** 上に出す案内 (記録がほかで変わって出し直したとき)。 */
		notice?: string;
		/** 変わるたびに既存の記録を取り直す。 */
		reloadKey?: number;
		/** 左の「戻る」。見出しの戻るボタンと同じく、さっきの画面へ戻す。 */
		onback: () => void;
		/** 差分を確認して確定したとき。確認画面に出した既存の記録を渡す。取り込みを実行し、成功した後の
		 * 移動は呼び出し側。 */
		onconfirm: (existing: ExistingEntry[]) => void;
	} = $props();

	let range = $derived(importDateRange(rows));
	/** `$effect` は `range` (行が変わるたびに新しいオブジェクトになる) ではなく、この文字列だけを読む。
	 * 日付の範囲が実際に変わったときだけ既存の記録を取り直すため。 */
	let rangeFrom = $derived(range?.from ?? null);
	let rangeTo = $derived(range?.to ?? null);

	/** 対象行の日付の範囲にある既存の記録。置き換えるのはこのうち対象行がある日だけなので
	 * (`buildImportDiff` が絞る)、ここでは範囲でまとめて取る。 */
	let existing = $state<
		| { status: 'loading' }
		| { status: 'error'; message: string }
		| { status: 'ready'; entries: ExistingEntry[] }
	>({ status: 'loading' });

	$effect(() => {
		const from = rangeFrom;
		const to = rangeTo;
		void reloadKey;
		if (from === null || to === null) return;

		let cancelled = false;
		existing = { status: 'loading' };
		fetchExistingEntries({ from, to }).then((result) => {
			if (cancelled) return;
			existing = result.ok
				? { status: 'ready', entries: result.entries }
				: { status: 'error', message: result.message };
		});
		return () => {
			cancelled = true;
		};
	});

	let groups = $derived(existing.status === 'ready' ? buildImportDiff(rows, existing.entries) : []);
	let warning = $derived(deletionWarning(groups));

	/** 確認画面に差分を出せていない間は確定させない (見ていない内容で削除が走らないように)。 */
	let canConfirm = $derived(range !== null && existing.status === 'ready' && !importing);

	function confirmImport() {
		if (!canConfirm || existing.status !== 'ready') return;
		onconfirm(existing.entries);
	}

	const STATUS_LABELS: Record<ImportDiffStatus, () => string> = {
		added: m.import_preview_status_added,
		removed: m.import_preview_status_removed,
		unchanged: m.import_preview_status_unchanged
	};

	const STATUS_PILL_CLASSES: Record<ImportDiffStatus, string> = {
		added: 'bg-secondary text-secondary-foreground',
		removed: 'bg-destructive/10 text-destructive',
		unchanged: 'bg-background text-muted-foreground'
	};

	function timeOf(row: ImportDiffRow): string {
		return row.measuredAtLocal.slice(11, 16);
	}
</script>

{#if notice || warning || importError}
	<div class="flex shrink-0 flex-col gap-2 px-5 pb-3">
		{#if notice}
			<NoticeBox variant="muted" message={notice} />
		{/if}
		{#if warning}
			<NoticeBox variant="destructive" message={warning} />
		{/if}
		{#if importError}
			<LoadError message={importError} />
		{/if}
	</div>
{/if}

<div class="flex-1 px-5 pb-5">
	{#if existing.status === 'loading'}
		<LoadingIndicator />
	{:else if existing.status === 'error'}
		<LoadError message={existing.message} />
	{:else}
		<div class="flex flex-col gap-3.5">
			{#each groups as group (group.date)}
				<section>
					<h3
						class={[
							'mb-2 text-sm font-bold',
							group.hasRemoval ? 'text-destructive' : 'text-muted-foreground'
						]}
					>
						{formatDateWithWeekday(parseLocalDate(group.date))}
					</h3>
					<ul
						class={[
							'divide-y overflow-hidden rounded-md raised bg-card',
							group.hasRemoval && 'border border-destructive/15 dark:border-destructive/40'
						]}
					>
						{#each group.rows as row, i (i)}
							<li class="flex items-center justify-between gap-3 px-3.5 py-3">
								<div
									class={[
										'flex min-w-0 flex-wrap items-baseline gap-x-3 text-sm',
										row.status !== 'added' && 'text-muted-foreground',
										row.status === 'removed' && 'line-through'
									]}
								>
									<span>{timeOf(row)}</span>
									<span>{row.systolic} / {row.diastolic}</span>
									{#if row.pulse !== undefined}
										<span>{m.record_pulse_with_value({ pulse: row.pulse })}</span>
									{/if}
									{#if row.memo}
										<span class="min-w-0 break-words">{row.memo}</span>
									{/if}
								</div>
								<span
									class={[
										'shrink-0 rounded-full px-2.5 py-0.5 text-xs font-bold',
										STATUS_PILL_CLASSES[row.status]
									]}
								>
									{STATUS_LABELS[row.status]()}
								</span>
							</li>
						{/each}
					</ul>
				</section>
			{/each}
		</div>
	{/if}
</div>

<ScreenFooter>
	<Button
		type="button"
		variant="outline"
		class="flex-1 text-muted-foreground"
		disabled={importing}
		onclick={onback}
	>
		{m.common_back_button()}
	</Button>
	<LoadingButton
		type="button"
		class="flex-1 shadow-soft dark:shadow-none"
		loading={importing}
		disabled={!canConfirm}
		onclick={confirmImport}
	>
		{m.import_preview_confirm_button()}
	</LoadingButton>
</ScreenFooter>
