<script lang="ts">
	import { onDestroy, onMount } from 'svelte';
	import { goto } from '$app/navigation';
	import { resolve } from '$app/paths';
	import { GENERIC_ERROR_MESSAGE } from '$lib/api/errors';
	import { userTimeZone } from '$lib/auth';
	import { currentYearIn } from '$lib/period';
	import { LatestRequest } from '$lib/latest-request';
	import {
		CSV_FILE_ACCEPT,
		importDateRange,
		importRowNote,
		importRowStatus,
		decodeCsvText,
		parseCsvImport,
		importFileHandoff,
		type ImportRow
	} from '$lib/import';
	import { ImportConfirmation } from '$lib/import-confirmation.svelte';
	import {
		atPreviewStep,
		backFromPreview,
		clearPreviewStep,
		leaveImportPage,
		pushPreviewStep,
		setImportResult
	} from '$lib/import-step';
	import { LeaveGuard } from '$lib/leave-guard.svelte';
	import * as m from '$lib/paraglide/messages.js';
	import { openFilePicker } from '$lib/utils';
	import {
		fetchPeriodThresholds,
		DEFAULT_PERIOD_THRESHOLDS,
		type PeriodThresholds
	} from '$lib/settings';
	import { Button } from '$lib/components/ui/button';
	import ImportRows from '$lib/components/import-rows.svelte';
	import ImportPreview from '$lib/components/import-preview.svelte';
	import ImportRowSheet from '$lib/components/import-row-sheet.svelte';
	import LeaveConfirmDialog from '$lib/components/leave-confirm-dialog.svelte';
	import LoadError from '$lib/components/load-error.svelte';
	import LoadingIndicator from '$lib/components/loading-indicator.svelte';
	import PageHeader from '$lib/components/page-header.svelte';
	import ScreenFooter from '$lib/components/screen-footer.svelte';

	/** `pick` はファイルがまだ無い (再読み込み・URL を直接開いた)、`rows` は読み込めた。 */
	type Stage = 'pick' | 'reading' | 'error' | 'rows';

	let stage = $state<Stage>('pick');
	let errorText = $state('');
	let rows = $state<ImportRow[]>([]);
	/** 朝/夜の時間帯。行の日時に「朝」「夜」を添えるのに使う。 */
	let periods = $state<PeriodThresholds>(DEFAULT_PERIOD_THRESHOLDS);

	let rowSheet = $state<ReturnType<typeof ImportRowSheet> | null>(null);

	/** 行の値を直したか。直していれば、離れる前に確認する。 */
	let edited = false;
	/** 設定画面からファイルを受け取って開いたか。そのときは戻る・完了で履歴を戻り、
	 * 取り込みのページを履歴に残さない。 */
	let fromSettings = false;

	let fileInput = $state<HTMLInputElement | null>(null);
	/** 読み込みの世代。古い読み込みの結果で state を上書きしないため。 */
	const importRequest = new LatestRequest();

	/** 対象行の日付の範囲。対象行が1件も無ければ `null` (確認へ進めない)。 */
	let range = $derived(importDateRange(rows));
	/** 確認の段階か。 */
	let previewing = $derived(stage === 'rows' && atPreviewStep());

	const guard = new LeaveGuard(
		() => edited,
		() => confirmation.importing
	);
	const confirmation = new ImportConfirmation((created) => {
		// 確定できたのは対象行があったとき (`range` が `null` なら確認へ進めない)。
		if (range) setImportResult({ ...range, created });
		guard.allow();
		leave();
	});

	onMount(() => {
		const file = importFileHandoff.take();
		if (!file) return;
		fromSettings = true;
		load(file);
	});

	onDestroy(() => {
		importRequest.cancel();
		confirmation.reset();
	});

	function pickFile() {
		openFilePicker(fileInput);
	}

	function handleFileChange(e: Event) {
		const file = (e.currentTarget as HTMLInputElement).files?.[0];
		if (file) load(file);
	}

	async function load(file: File) {
		const isLatest = importRequest.begin();
		// 確認の段階の履歴で再読み込みした後に選び直しても、編集の段階から始める。
		clearPreviewStep();
		stage = 'reading';
		try {
			// 朝/夜の時間帯は、時刻の無い行に仮時刻を振るのと表示に使う。取れなければ既定値で進める
			// (`fetchPeriodThresholds` は失敗しない)。
			const [bytes, thresholds] = await Promise.all([file.arrayBuffer(), fetchPeriodThresholds()]);
			if (!isLatest()) return;
			periods = thresholds;
			const result = parseCsvImport(decodeCsvText(bytes), userTimeZone(), periods);
			if ('error' in result) {
				fail(result.error);
				return;
			}
			if (result.rows.length === 0) {
				fail(m.import_page_no_records_error());
				return;
			}
			rows = result.rows;
			stage = 'rows';
		} catch {
			// file.arrayBuffer() の失敗 (ピッカーで選んだ後にファイルが移動/削除された等)。
			if (isLatest()) fail(GENERIC_ERROR_MESSAGE());
		}
	}

	function fail(message: string) {
		errorText = message;
		stage = 'error';
	}

	/** 取り込みのページを離れて設定画面へ戻る。 */
	function leave() {
		leaveImportPage(fromSettings, () => goto(resolve('/settings')));
	}

	function back() {
		backFromPreview(previewing, leave);
	}
</script>

<input
	bind:this={fileInput}
	type="file"
	accept={CSV_FILE_ACCEPT}
	class="hidden"
	onchange={handleFileChange}
/>

<!-- ファイルを選ぶ前後で続けて使うので、見出しは設定画面と同じ PageHeader にし、
     下端のボタンの帯を画面の下に留める。 -->
<div class="mx-auto flex min-h-dvh max-w-2xl flex-col">
	<div class="px-5">
		<PageHeader
			backDisabled={confirmation.importing}
			onback={back}
			title={previewing ? m.import_preview_title() : m.import_page_title()}
		/>
	</div>

	{#if previewing}
		<ImportPreview
			importing={confirmation.importing}
			importError={confirmation.error}
			notice={confirmation.notice}
			reloadKey={confirmation.reloadKey}
			{rows}
			onback={back}
			onconfirm={(existing) => confirmation.confirm(rows, existing)}
		/>
	{:else}
		<div class="flex flex-1 flex-col gap-4 px-5 pb-5">
			{#if stage === 'pick'}
				<p class="text-sm leading-relaxed text-muted-foreground">
					{m.import_description()}
				</p>
				<Button type="button" onclick={pickFile}>{m.import_page_pick_file_button()}</Button>
			{:else if stage === 'reading'}
				<LoadingIndicator />
			{:else if stage === 'error'}
				<LoadError message={errorText} />
				<Button type="button" onclick={pickFile}>{m.import_page_pick_file_button()}</Button>
			{:else}
				<ImportRows
					bind:rows
					{periods}
					thisYear={currentYearIn(userTimeZone())}
					statusOf={importRowStatus}
					noteOf={importRowNote}
					onedit={(row) => rowSheet?.show(row)}
					onkeepchange={() => (edited = true)}
				/>
			{/if}
		</div>

		{#if stage === 'rows'}
			<ScreenFooter>
				<Button type="button" variant="outline" class="flex-1 text-muted-foreground" onclick={back}>
					{m.common_back_button()}
				</Button>
				<Button
					type="button"
					class="flex-1 shadow-soft dark:shadow-none"
					disabled={range === null}
					onclick={pushPreviewStep}
				>
					{m.common_review_button()}
				</Button>
			</ScreenFooter>
		{/if}
	{/if}
</div>

<ImportRowSheet
	bind:this={rowSheet}
	idPrefix="import-row"
	statusOf={importRowStatus}
	onapplied={() => (edited = true)}
/>

<LeaveConfirmDialog {guard} />
