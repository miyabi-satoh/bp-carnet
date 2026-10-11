<script lang="ts">
	import { onDestroy, onMount } from 'svelte';
	import { goto } from '$app/navigation';
	import { resolve } from '$app/paths';
	import { GENERIC_ERROR_MESSAGE } from '$lib/api/errors';
	import { AppleHealthError, readAppleHealthExport } from '$lib/apple-health';
	import { userTimeZone } from '$lib/auth';
	import { currentYearIn, localToday } from '$lib/period';
	import { LatestRequest } from '$lib/latest-request';
	import {
		IMPORT_FILE_ACCEPT,
		importDateRange,
		importFileType,
		importRowNote,
		importRowStatus,
		decodeCsvText,
		parseCsvImport,
		importFileHandoff,
		type ImportRow,
		type ImportRowStatus
	} from '$lib/import';
	import { parseTableImport, tableSample } from '$lib/import-table';
	import {
		fetchOcrStatus,
		memoRowNote,
		memoRowStatus,
		memoRowsFromReadings,
		quotasExhausted,
		type MemoRow
	} from '$lib/ocr';
	import { DOCUMENT_MAX_BYTES, fetchTableColumns, PDF_MAGIC, readDocument } from '$lib/ocr-files';
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
	import FileReadConsentDialog from '$lib/components/file-read-consent-dialog.svelte';
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
	/** 読み込み中に出す文言。時間のかかる読み込み (「ヘルスケア」の書き出し・Gemini の読み取り) だけ、何をしているかを出す。 */
	let readingLabel = $state<string | undefined>(undefined);
	/** PDF から読み取った行は、読み取りの自信も持つ (`MemoRow`)。 */
	let rows = $state<(ImportRow | MemoRow)[]>([]);
	/** 朝/夜の時間帯。行の日時に「朝」「夜」を添えるのに使う。 */
	let periods = $state<PeriodThresholds>(DEFAULT_PERIOD_THRESHOLDS);

	let rowSheet = $state<ReturnType<typeof ImportRowSheet> | null>(null);

	/** 行の値を直したか。直していれば、離れる前に確認する。 */
	let edited = false;
	/** 設定画面からファイルを受け取って開いたか。そのときは戻る・完了で履歴を戻り、
	 * 取り込みのページを履歴に残さない。 */
	let fromSettings = false;

	let fileInput = $state<HTMLInputElement | null>(null);

	let consentDialog = $state<ReturnType<typeof FileReadConsentDialog> | null>(null);
	/** 同意を待っている読み取り。同意したら実行し、断ったら捨てる。 */
	let pendingRead: (() => Promise<void>) | null = null;
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

	/** 行の状態。PDF から読み取った行は、読み取りの自信も見る (写真の手帳の読み取りと同じ)。 */
	function statusOf(row: ImportRow | MemoRow): ImportRowStatus {
		return 'confidence' in row ? memoRowStatus(row) : importRowStatus(row);
	}

	function noteOf(row: ImportRow | MemoRow): string | undefined {
		return 'confidence' in row ? memoRowNote(row) : importRowNote(row);
	}

	async function load(file: File) {
		const isLatest = importRequest.begin();
		// 確認の段階の履歴で再読み込みした後に選び直しても、編集の段階から始める。
		clearPreviewStep();
		pendingRead = null;
		readingLabel = undefined;
		stage = 'reading';
		try {
			// 朝/夜の時間帯は、時刻の無い行に仮時刻を振るのと表示に使う。取れなければ既定値で進める
			// (`fetchPeriodThresholds` は失敗しない)。
			periods = await fetchPeriodThresholds();
			if (!isLatest()) return;
			switch (importFileType(file)) {
				case 'appleHealth':
					await loadAppleHealth(file, isLatest);
					break;
				case 'document':
					await loadDocument(file, isLatest);
					break;
				case 'table':
					await loadTable(file, isLatest);
					break;
			}
		} catch {
			// file.arrayBuffer() の失敗 (ピッカーで選んだ後にファイルが移動/削除された等)。
			if (isLatest()) fail(GENERIC_ERROR_MESSAGE());
		}
	}

	/** 読み込んだ行を一覧に出す。1件も無ければ、その旨を出す。 */
	function showRows(loaded: (ImportRow | MemoRow)[]) {
		if (loaded.length === 0) {
			fail(m.import_page_no_records_error());
			return;
		}
		rows = loaded;
		stage = 'rows';
	}

	/** Gemini の読み取りを今は使えない理由。使えるなら `undefined`。同意を求める前に確かめる
	 * (同意して送った後に、使えないと断られないように)。`disabledMessage` は、読み取りが無効なときの文。 */
	async function readBlockedReason(disabledMessage: string): Promise<string | undefined> {
		const status = await fetchOcrStatus();
		// 状態を取れなかった (通信の失敗など) のは、無効とは違う。
		if (status.enabled === null) return GENERIC_ERROR_MESSAGE();
		if (!status.enabled) return disabledMessage;
		if (quotasExhausted(status.quotas)) return m.error_ocr_budget_exhausted();
		return undefined;
	}

	/** 表の形のテキスト。このアプリが書き出した CSV はそのまま読み、ほかは列の意味を Gemini に決めてもらう
	 * (docs/import-export.md「他のアプリのファイル」)。 */
	async function loadTable(file: File, isLatest: () => boolean) {
		const text = decodeCsvText(await file.arrayBuffer());
		if (!isLatest()) return;
		const own = parseCsvImport(text, userTimeZone(), periods);
		if (!('error' in own)) {
			showRows(own.rows);
			return;
		}
		const sample = tableSample(text);
		if (sample === '') {
			fail(own.error);
			return;
		}
		// 読み取りが無効なら、このアプリの CSV でないことだけを伝える。
		const blocked = await readBlockedReason(own.error);
		if (!isLatest()) return;
		if (blocked !== undefined) {
			fail(blocked);
			return;
		}
		askConsent('table', async () => {
			const result = await fetchTableColumns(sample);
			if (!isLatest()) return;
			if (!result.ok) fail(result.message);
			else if (!result.data.readable) fail(m.import_error_table_unreadable());
			else showRows(parseTableImport(text, result.data, userTimeZone(), periods));
		});
	}

	/** PDF。全体を Gemini に送って読み取ってもらう。 */
	async function loadDocument(file: File, isLatest: () => boolean) {
		// 送れない大きさのファイルは、同意を求める前に伝える (同意して全体を送った後に断られないように)。
		if (file.size > DOCUMENT_MAX_BYTES) {
			fail(m.error_ocr_file_too_large());
			return;
		}
		// 中身が PDF でないファイルも、送る前に伝える。
		const head = new Uint8Array(await file.slice(0, PDF_MAGIC.length).arrayBuffer());
		if (!isLatest()) return;
		if (!PDF_MAGIC.every((byte, i) => head[i] === byte)) {
			fail(m.error_ocr_unsupported_file());
			return;
		}
		const blocked = await readBlockedReason(m.error_ocr_disabled());
		if (!isLatest()) return;
		if (blocked !== undefined) {
			fail(blocked);
			return;
		}
		askConsent('document', async () => {
			const result = await readDocument(file);
			if (!isLatest()) return;
			if (!result.ok) fail(result.message);
			else showRows(memoRowsFromReadings(result.data, periods, localToday(userTimeZone())));
		});
	}

	/** 「ヘルスケア」の書き出し。Gemini に送らず、この端末の中で読む。 */
	async function loadAppleHealth(file: File, isLatest: () => boolean) {
		const progress = (fraction: number) => {
			if (isLatest()) {
				readingLabel = m.import_page_reading_file({ percent: Math.floor(fraction * 100) });
			}
		};
		progress(0);
		try {
			const loaded = await readAppleHealthExport(file, userTimeZone(), progress);
			if (isLatest()) showRows(loaded);
		} catch (error) {
			if (!(error instanceof AppleHealthError)) throw error;
			if (!isLatest()) return;
			fail(
				error.reason === 'unsupported'
					? m.import_error_apple_health_unsupported()
					: m.import_error_apple_health_not_export()
			);
		}
	}

	/** ファイルを Gemini に送る前に、送るものを示して同意を得る。同意したら `read` を実行する。 */
	function askConsent(kind: 'table' | 'document', read: () => Promise<void>) {
		pendingRead = read;
		consentDialog?.show(kind);
	}

	async function runPendingRead() {
		const read = pendingRead;
		pendingRead = null;
		if (!read) return;
		readingLabel = m.import_page_reading_by_ai();
		try {
			await read();
		} catch {
			fail(GENERIC_ERROR_MESSAGE());
		}
	}

	/** 同意しなかったら、ファイルを選ぶところへ戻る。 */
	function cancelPendingRead() {
		pendingRead = null;
		stage = 'pick';
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
	accept={IMPORT_FILE_ACCEPT}
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
				<LoadingIndicator label={readingLabel} />
			{:else if stage === 'error'}
				<LoadError message={errorText} />
				<Button type="button" onclick={pickFile}>{m.import_page_pick_file_button()}</Button>
			{:else}
				<ImportRows
					bind:rows
					{periods}
					thisYear={currentYearIn(userTimeZone())}
					{statusOf}
					{noteOf}
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
	{statusOf}
	onapplied={() => (edited = true)}
/>

<FileReadConsentDialog
	bind:this={consentDialog}
	onagreed={runPendingRead}
	ondeclined={cancelPendingRead}
/>

<LeaveConfirmDialog {guard} />
