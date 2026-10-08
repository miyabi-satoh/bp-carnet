<script lang="ts">
	import { onDestroy, onMount } from 'svelte';
	import { goto } from '$app/navigation';
	import { resolve } from '$app/paths';
	import { GENERIC_ERROR_MESSAGE, type ErrorCode } from '$lib/api/errors';
	import { userTimeZone } from '$lib/auth';
	import { fetchExistingEntries, importDateRange } from '$lib/import';
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
	import {
		carryOverMemos,
		confidenceLevel,
		extractFromPhoto,
		fetchOcrStatus,
		memoRowNote,
		memoRowStatus,
		memoRowsFromReadings,
		memoRowsYear,
		PHOTO_FILE_ACCEPT,
		pulseFromReading,
		quotasExhausted,
		shiftMemoRowsYear,
		photoFileHandoff,
		type MemoRow,
		type OcrQuota,
		type OcrReading
	} from '$lib/ocr';
	import {
		DEFAULT_PERIOD_THRESHOLDS,
		fetchPeriodThresholds,
		type PeriodThresholds
	} from '$lib/settings';
	import * as m from '$lib/paraglide/messages.js';
	import { currentYearIn, localToday, toLocalDateTime } from '$lib/period';
	import { LatestRequest } from '$lib/latest-request';
	import { photoTakenAt } from '$lib/photo-taken-at';
	import { recordFormSubmitLabel, requestManualEntry, splitMeasuredAt } from '$lib/record-form';
	import { hasSameRecord, saveRecord, type CreateRecordRequest } from '$lib/records';
	import { openFilePicker } from '$lib/utils';
	import ButtonRow from '$lib/components/button-row.svelte';
	import ImportPreview from '$lib/components/import-preview.svelte';
	import LeaveConfirmDialog from '$lib/components/leave-confirm-dialog.svelte';
	import LoadingButton from '$lib/components/loading-button.svelte';
	import LoadingIndicator from '$lib/components/loading-indicator.svelte';
	import LoadError from '$lib/components/load-error.svelte';
	import OcrQuotaCard from '$lib/components/ocr-quota-card.svelte';
	import NoticeDialog from '$lib/components/notice-dialog.svelte';
	import OcrConfidence from '$lib/components/ocr-confidence.svelte';
	import OcrConsentDialog from '$lib/components/ocr-consent-dialog.svelte';
	import ImportRows from '$lib/components/import-rows.svelte';
	import ImportRowSheet from '$lib/components/import-row-sheet.svelte';
	import PageHeader from '$lib/components/page-header.svelte';
	import PhotoLightbox from '$lib/components/photo-lightbox.svelte';
	import PhotoPreview from '$lib/components/photo-preview.svelte';
	import RecordForm from '$lib/components/record-form.svelte';
	import SameRecordDialog from '$lib/components/same-record-dialog.svelte';
	import ScreenFooter from '$lib/components/screen-footer.svelte';
	import TopupConfirmDialog from '$lib/components/topup-confirm-dialog.svelte';
	import { Button } from '$lib/components/ui/button';
	import { FIELD_SELECT_CLASS } from '$lib/styles';

	/** `pick` は写真がまだ無い (再読み込み・URL を直接開いた)、`confirm` は選んだ写真を読み取る前に確かめている、
	 * `result` は読み取りが終わった。 */
	type Stage = 'pick' | 'confirm' | 'reading' | 'error' | 'result';

	let stage = $state<Stage>('pick');
	/** 読み取りの枠を使う順に並べたもの。無制限・無効・取得できないときは `null` で、残りの枠を出さない (docs/ocr.md)。 */
	let quotas = $state<OcrQuota[] | null>(null);
	/** 全部の枠を足した残りが少ないか。少なければゲージを注意の色にする。 */
	let quotaLow = $state(false);
	/** 読み取りを買い足せるか (Stripe 有効で、無料の枠に上限がある)。「読み取りを買い足す」は残りの量によらず出す
	 * (量で出し消しすると、買った直後に消えて不具合に見えるため。docs/ocr.md)。 */
	let topupAvailable = $state(false);
	/** 読み取りが無効と分かっている (`enabled: false`)。取得できないときは無効とみなさない (サーバーが読み取りの段階で断る)。 */
	let ocrDisabled = $state(false);
	let topupDialog = $state<ReturnType<typeof TopupConfirmDialog> | null>(null);
	/** 写真を Gemini API に送ることに同意しているか。分からない (取得できない) ときは `null` で、
	 * 同意済みとは扱わない (読み取る前に聞く)。 */
	let consented = $state<boolean | null>(null);
	let consentDialog = $state<ReturnType<typeof OcrConsentDialog> | null>(null);

	/** 遅れて返った古い応答で、新しい値を上書きしない。 */
	const ocrStatusRequest = new LatestRequest();
	function refreshOcrStatus() {
		const isLatest = ocrStatusRequest.begin();
		void fetchOcrStatus().then((status) => {
			if (!isLatest()) return;
			ocrDisabled = status.enabled === false;
			quotas = status.quotas;
			quotaLow = status.quotaLow;
			topupAvailable = status.topupAvailable;
			// 取り直しに失敗しても、同意したことは忘れない。
			if (status.consented !== null) consented = status.consented;
		});
	}
	/** アプリで買い足したとき (ウェブは Stripe へ移り、結果の画面から戻る)。使い切りで読み取れなかった後なら、
	 * 選んだ写真のまま「読み取る」の段階に戻す。 */
	function afterTopup() {
		if (stage === 'error' && errorCode === 'ocr_budget_exhausted' && photoFile) {
			errorCode = undefined;
			errorBlocksReading = false;
			stage = 'confirm';
		}
		refreshOcrStatus();
	}
	let lightbox = $state<ReturnType<typeof PhotoLightbox> | null>(null);
	/** 上に留めた見出しの帯 (写真の枠を含む) の高さ。 */
	let headerHeight = $state(0);

	// 入力エラーの欄へ移るスクロールで、欄が帯の下に隠れないようにする。
	$effect(() => {
		const root = document.documentElement;
		root.style.scrollPaddingTop = `${headerHeight}px`;
		return () => {
			root.style.scrollPaddingTop = '';
		};
	});

	let errorText = $state('');
	let errorCode = $state<ErrorCode | undefined>(undefined);
	let errorBlocksReading = $state(false);
	/** 読み取りに失敗し、写真を選び直しても読み取れない (`PhotoReadFailure.blocked`、または残りが 0%)。
	 * このときは「もう一度」を出さず、数字で入力する導線だけを出す (docs/mobile-app.md)。 */
	let readingBlocked = $derived(
		stage === 'error' && (errorBlocksReading || quotasExhausted(quotas))
	);
	/** 全部の枠を使い切った。残量の取り直しが間に合わなくても、429 を受けたら使い切りとして出す。 */
	let outOfQuota = $derived(
		quotasExhausted(quotas) || (stage === 'error' && errorCode === 'ocr_budget_exhausted')
	);
	/** 今使っている枠 (使う順で最初の、残りのある枠)。 */
	let currentQuotaIndex = $derived(quotas?.findIndex((quota) => quota.remainingPercent > 0) ?? -1);
	/** 読み取りが無効か、無料枠を使い切った後、写真を選ぶ前・読み取る前の段階で、数字で入力する導線を出すか (docs/ocr.md)。 */
	let offerManualEntry = $derived(
		(ocrDisabled || quotasExhausted(quotas)) && (stage === 'pick' || stage === 'confirm')
	);

	/** 読み取った値・行を直したか (チェックの付け外しを含む)。直していれば、離れる前に確認する。 */
	let edited = false;
	/** ホーム画面から写真を受け取って開いたか。そのときは戻る・完了で履歴を戻り、
	 * 写真で記録のページを履歴に残さない。 */
	let fromHome = false;

	let fileInput = $state<HTMLInputElement | null>(null);
	/** 読み取りの世代。古い応答で state を上書きしないため。 */
	const ocrRequest = new LatestRequest();

	/** 読み取りに使った写真のプレビュー (object URL)。差し替え・破棄のときに解放する。 */
	let photoUrl = $state<string | null>(null);
	/** 選んだ写真。「読み取る」を押すまで読み取らない (docs/ocr.md)。 */
	let photoFile: File | null = null;

	function setPhoto(file: File | null) {
		if (photoUrl) URL.revokeObjectURL(photoUrl);
		photoUrl = file ? URL.createObjectURL(file) : null;
	}

	/** 血圧値が読み取れたときの結果。`null` なら "monitor_none"/"unknown"/reading 無しを表す。 */
	let reading = $state<OcrReading | null>(null);
	/** reading が無かった場合に添える、何が写っていたかに応じた案内。Gemini の `notes` はそのまま
	 * 出さない (言い回しが一定せず、英語で返ることもあるため)。 */
	let emptyResultNote = $state('');

	/** 手書きメモの複数行読み取り結果。`null` なら単発読み取り結果 (`reading`) を表示中。 */
	let memoRows = $state<MemoRow[] | null>(null);

	/** 対象行の日付の範囲。取り込める行が1件も無ければ `null` (確認へ進めない)。 */
	let memoRange = $derived(importDateRange(memoRows ?? []));

	/** 取り込み内容の確認の段階か。採用行すべてを編集カードで並べる段階に差分も並べると、削除の警告が
	 * カードの下に埋もれてスクロールしないと見えなくなるため段階を分ける。 */
	let previewing = $derived(stage === 'result' && memoRows !== null && atPreviewStep());

	const guard = new LeaveGuard(
		() => edited,
		() => memoConfirmation.importing || submitting
	);
	const memoConfirmation = new ImportConfirmation((created) => {
		// 確定できたのは対象行があったとき (`memoRange` が `null` なら確認へ進めない)。
		if (memoRange) setImportResult({ ...memoRange, created });
		guard.allow();
		leave();
	});

	/** 血圧計の読み取り結果を入れて登録するフォームの入力値。記録フォームと同じく入力されたままの
	 * 文字列で持つ。 */
	let resultDate = $state('');
	let resultTime = $state('');
	let resultSystolic = $state('');
	let resultDiastolic = $state('');
	let resultPulse = $state('');
	let resultMemo = $state('');
	let submitting = $state(false);
	let errorDialog = $state<ReturnType<typeof NoticeDialog> | null>(null);
	let sameRecordDialog = $state<ReturnType<typeof SameRecordDialog> | null>(null);

	/** 朝/夜の時間帯。時刻なしの行に仮時刻を振るのと、一覧の日時に「朝」「夜」を添えるのに使う。
	 * `runOcr` のたびに取得し直す。 */
	let periodThresholds = $state<PeriodThresholds>(DEFAULT_PERIOD_THRESHOLDS);

	/** メモの引き継ぎを待っている行 (`showMemoRows`)。`memoRows` と同じ間は確認へ進ませない
	 * (確認した内容と、後から引き継いだ送信内容を食い違わせないため)。 */
	let carryingOverRows = $state.raw<MemoRow[] | null>(null);

	let memoSheet = $state<ReturnType<typeof ImportRowSheet<MemoRow>> | null>(null);

	onMount(() => {
		refreshOcrStatus();
		const file = photoFileHandoff.take();
		if (!file) return;
		fromHome = true;
		selectPhoto(file);
	});

	onDestroy(() => {
		ocrStatusRequest.cancel();
		ocrRequest.cancel();
		setPhoto(null);
		memoConfirmation.reset();
	});

	function pickPhoto() {
		openFilePicker(fileInput);
	}

	function handleFileChange(e: Event) {
		const file = (e.currentTarget as HTMLInputElement).files?.[0];
		if (file) selectPhoto(file);
	}

	/** 選んだ写真を見せ、「読み取る」を待つ。前の読み取り結果は捨てる。 */
	function selectPhoto(file: File) {
		ocrRequest.cancel();
		// 確認の段階の履歴で再読み込みした後に選び直しても、編集の段階から始める。
		clearPreviewStep();
		carryingOverRows = null;
		// 選び直した写真の読み取りで、直した値は置き換わる。
		edited = false;
		photoFile = file;
		setPhoto(file);
		stage = 'confirm';
	}

	function markEdited() {
		edited = true;
	}

	/** 読み取った行のメモ。年を選び直して引き継ぎ直すときに、引き継いだメモを読み取ったメモへ戻すのに使う。 */
	let readMemos = new Map<MemoRow, string>();
	/** 引き継いだメモ。これと同じままのメモだけを、引き継ぎ直すときに戻す (直したメモは残す)。 */
	let carriedMemos = new Map<MemoRow, string>();
	/** 引き継ぎの世代。年を選び直したら、前の年の範囲の応答で上書きしない。 */
	const carryRequest = new LatestRequest();
	/** 行を出した読み取りが、まだ最新か (写真を選び直した・ページを離れたら `false`)。 */
	let memoReadingIsLatest: () => boolean = () => false;

	/** 読み取った行を表示し、置き換えられる既存の記録からメモを引き継ぐ (`carryOverMemos`)。 */
	function showMemoRows(readings: OcrReading[], isLatest: () => boolean) {
		memoRows = memoRowsFromReadings(readings, periodThresholds, localToday(userTimeZone()));
		readMemos = new Map(memoRows.map((row) => [row, row.memo]));
		carriedMemos = new Map();
		memoReadingIsLatest = isLatest;
		void carryOver(memoRows);
	}

	/** 置き換えられる既存の記録からメモを引き継ぐ。引き継ぎは表示の後に非同期で反映する。
	 * 既存の記録を取得できなければ引き継がない (メモが消える記録は、確認の段階の削除の警告で気づける)。 */
	async function carryOver(rows: MemoRow[]) {
		const isLatestCarry = carryRequest.begin();
		const range = importDateRange(rows);
		if (range === null) {
			if (carryingOverRows === rows) carryingOverRows = null;
			return;
		}
		// 引き継ぐのは、読み取ったままのメモだけ (本人が画面で直したメモは上書きしない)。
		const memosBefore = new Map(rows.map((row) => [row, row.memo]));
		carryingOverRows = rows;
		const result = await fetchExistingEntries(range);
		if (!isLatestCarry()) return;
		if (carryingOverRows === rows) carryingOverRows = null;
		// 待っている間に読み直された・行が差し替えられた・ページを離れたら反映しない。
		if (!memoReadingIsLatest() || memoRows !== rows || !result.ok) return;
		// 取得を待つ間に直されたメモも上書きしない。
		const carriable = new Set(
			rows.filter((row) => row.memo === memosBefore.get(row) && row.memo === readMemos.get(row))
		);
		carryOverMemos(rows, result.entries, periodThresholds, (row) => carriable.has(row));
		carriedMemos = new Map(
			[...carriable].filter((row) => row.memo !== readMemos.get(row)).map((row) => [row, row.memo])
		);
	}

	/** 「年」の欄で年を選び直す (docs/ocr.md)。日付が変わるので、メモを引き継ぎ直す。 */
	function changeMemoYear(year: number) {
		if (!memoRows) return;
		shiftMemoRowsYear(memoRows, year);
		for (const [row, memo] of carriedMemos) {
			if (row.memo === memo) row.memo = readMemos.get(row) ?? '';
		}
		carriedMemos = new Map();
		markEdited();
		void carryOver(memoRows);
	}

	/** 「年」の欄で選べる、今年より前の年数。長年の手帳を取り込めるよう広めにとる。 */
	const MEMO_YEAR_SPAN = 30;
	/** 手書きメモの行の年。「年」の欄に出す。 */
	let memoYear = $derived(memoRows ? memoRowsYear(memoRows) : null);
	/** 「年」の欄の選択肢。今年から古いほうへ並べる。 */
	let memoYearOptions = $derived.by(() => {
		const thisYear = localToday(userTimeZone()).getFullYear();
		const oldest = Math.min(thisYear - MEMO_YEAR_SPAN, memoYear ?? thisYear);
		return Array.from({ length: thisYear - oldest + 1 }, (_, i) => thisYear - i);
	});

	function showReading(value: OcrReading) {
		reading = value;
		resultSystolic = String(value.systolic);
		resultDiastolic = String(value.diastolic);
		resultPulse = String(pulseFromReading(value.pulse) ?? '');
	}

	/** 「読み取る」。まだ同意していなければ、送る前に同意を求める (docs/ocr.md)。 */
	async function requestOcr() {
		// ページを開いてすぐ押すと、状態の取得が間に合っていない。同意済みの人に聞き直さないよう、待って決める。
		if (consented === null) {
			const status = await fetchOcrStatus();
			if (consented === null) consented = status.consented;
		}
		if (consented) {
			void runOcr();
			return;
		}
		consentDialog?.show();
	}

	function consentGiven() {
		consented = true;
		void runOcr();
	}

	async function runOcr() {
		const file = photoFile;
		if (!file) return;
		const isLatest = ocrRequest.begin();
		stage = 'reading';
		try {
			const [result, periods, takenAt] = await Promise.all([
				extractFromPhoto(file),
				fetchPeriodThresholds(),
				// 撮影日時が読めなくても読み取りは続ける (現在時刻にする)。
				photoTakenAt(file, userTimeZone()).catch(() => null)
			]);
			// 読み取りで枠が減った (失敗の回も、応答が届けば減る) ので、残りを取り直す。
			refreshOcrStatus();
			if (!isLatest()) return;
			periodThresholds = periods;

			if (!result.ok && result.code === 'ocr_consent_required') {
				// ほかの端末で取り消していた。写真はそのままにして、同意を求め直す。
				consented = false;
				stage = 'confirm';
				consentDialog?.show();
				return;
			}
			if (!result.ok) {
				errorText = result.message;
				errorCode = result.code;
				errorBlocksReading = result.blocked;
				stage = 'error';
				return;
			}
			const data = result.data;

			if (data.kind === 'monitor_bp' && data.reading) {
				// 日時は液晶の時計ではなく、写真の撮影日時を既定にする (血圧計の時計は必ずしも正確ではないため)。
				// 撮影日時が読めなければ現在時刻 (docs/ocr.md)。
				({ measuredOnDate: resultDate, time: resultTime } = splitMeasuredAt(
					takenAt ?? toLocalDateTime(new Date(), userTimeZone())
				));
				resultMemo = '';
				showReading(data.reading);
				memoRows = null;
			} else if (data.kind === 'memo' && data.readings && data.readings.length > 0) {
				reading = null;
				showMemoRows(data.readings, isLatest);
			} else {
				reading = null;
				memoRows = null;
				emptyResultNote =
					data.kind === 'monitor_none'
						? m.photo_page_no_values_monitor_none()
						: data.kind === 'unknown'
							? m.photo_page_no_values_unknown()
							: '';
			}
			stage = 'result';
		} catch {
			// API の失敗は `extractFromPhoto()` が結果で返し、`fetchPeriodThresholds()` は既定値に
			// 切り替えるので、ここに来るのは結果の反映中 (`userTimeZone()` 等) の想定外の例外。
			if (!isLatest()) return;
			errorText = GENERIC_ERROR_MESSAGE();
			errorCode = undefined;
			errorBlocksReading = false;
			stage = 'error';
		}
	}

	/** 読み取った値で記録を登録する。登録したら、履歴を積まずにホーム画面へ戻る (docs/ocr.md)。 */
	async function saveReading(record: CreateRecordRequest): Promise<boolean> {
		// 同じ写真を2回登録しかけたら確かめる。調べられなければ聞かずに登録する。
		const same = await hasSameRecord(record);
		if (same.ok && same.exists && !(await sameRecordDialog?.show())) return false;
		const result = await saveRecord(null, record);
		if (!result.ok) {
			errorDialog?.show(m.record_form_dialog_create_error_title(), result.message);
			return false;
		}
		const savedOn = record.localMeasuredAt.slice(0, 10);
		setImportResult({ from: savedOn, to: savedOn, created: 1 });
		return true;
	}

	/** シートで直した値を行に書き戻した後。 */
	function memoRowApplied(row: MemoRow) {
		// 本人が数字を見て更新したので、読み取りの自信の低さは残さない (残すと、確かめた行が
		// 「自信がありません」のまま橙で並び続ける)。
		row.confidence = 1;
		markEdited();
	}

	/** 読み取れないときの逃げ道。ホーム画面へ戻り、記録フォームを開く。 */
	function enterManually() {
		requestManualEntry();
		leave();
	}

	/** 写真で記録のページを離れてホーム画面へ戻る。 */
	function leave() {
		leaveImportPage(fromHome, () => goto(resolve('/')));
	}

	function back() {
		backFromPreview(previewing, leave);
	}
</script>

<input
	bind:this={fileInput}
	type="file"
	accept={PHOTO_FILE_ACCEPT}
	class="hidden"
	onchange={handleFileChange}
/>

<!-- 読み取れない・読み取りに失敗したときの次の一手 (数字で入力する・使い方のページ)。
     `retry` なら「もう一度」を主な操作として右に並べる。 -->
{#snippet fallbackLinks(retry: boolean)}
	<div class="flex flex-col items-center gap-2 text-center text-sm">
		<ButtonRow class="w-full">
			<Button type="button" variant="outline" onclick={enterManually}>
				{m.photo_page_manual_entry_button()}
			</Button>
			{#if retry}
				<Button type="button" onclick={pickPhoto}>{m.common_again_button()}</Button>
			{/if}
		</ButtonRow>
		<a
			href={resolve('/help/[slug]', { slug: 'trouble-photo' })}
			class="inline-flex min-h-11 items-center text-accent-foreground underline hover:text-primary"
			>{m.help_trouble_photo_title()}</a
		>
	</div>
{/snippet}

{#snippet retakeButton(primary: boolean)}
	<Button
		type="button"
		variant={primary ? 'default' : 'outline'}
		class={['flex-1', !primary && 'text-muted-foreground']}
		disabled={submitting}
		onclick={pickPhoto}
	>
		{m.photo_page_retake_button()}
	</Button>
{/snippet}

{#snippet topupButton()}
	<Button type="button" variant="outline" onclick={() => topupDialog?.show()}>
		{m.photo_page_topup_button()}
	</Button>
{/snippet}

<!-- 使い切った後 (写真を選んでも読み取れない) の、買い足しと数字で入力する。 -->
{#snippet exhaustedActions()}
	<ButtonRow>
		{#if topupAvailable && outOfQuota}
			{@render topupButton()}
		{/if}
		<Button type="button" onclick={enterManually}>
			{m.photo_page_manual_entry_button()}
		</Button>
	</ButtonRow>
{/snippet}

<!-- 取り込みのページと同じく、見出しは PageHeader にし、下端のボタンの帯を画面の下に留める。 -->
<div class="mx-auto flex min-h-dvh max-w-2xl flex-col">
	<!-- 見出しの帯と写真の枠はスクロールしても上に残し、写真を見ながら一覧・フォームを確かめられるようにする (docs/ocr.md)。
	     PageHeader が上端の帯の余白を持つので top-safe にはしない (帯を地の色で覆う)。 -->
	<div class="sticky top-0 z-10 bg-background px-5" bind:clientHeight={headerHeight}>
		<PageHeader
			backDisabled={memoConfirmation.importing || submitting}
			onback={back}
			title={previewing ? m.import_preview_title() : m.photo_page_title()}
		/>
		{#if !previewing && photoUrl && (stage === 'confirm' || stage === 'reading' || stage === 'result')}
			<!-- 確かめる段階では、写真の全体が見えるよう大きな枠で縮めて出す。 -->
			{#key `${photoUrl}:${stage === 'confirm'}`}
				<PhotoPreview
					class={stage === 'confirm' ? 'mb-4 h-80' : 'mb-4 h-36'}
					startZoomed={stage !== 'confirm'}
					src={photoUrl}
					alt={m.photo_page_preview_alt()}
					onopen={(from) => lightbox?.show(from)}
				/>
			{/key}
		{/if}
	</div>

	{#if previewing}
		<ImportPreview
			importing={memoConfirmation.importing}
			importError={memoConfirmation.error}
			notice={memoConfirmation.notice}
			reloadKey={memoConfirmation.reloadKey}
			rows={memoRows ?? []}
			onback={back}
			onconfirm={(existing) => memoConfirmation.confirm(memoRows ?? [], existing)}
		/>
	{:else}
		<div class="flex flex-1 flex-col gap-4 px-5 pb-5">
			<!-- 読み取りの枠の残りは、今使っている枠を量によらず同じ枠で出す (docs/ocr.md)。使い切り (429 の失敗を含む) も
			     同じ枠の色と文言を替える。写真を選ぶ・確かめる・失敗の段階だけで、読み取り中と結果には出さない。 -->
			{#if (outOfQuota || quotas !== null) && (stage === 'pick' || stage === 'confirm' || stage === 'error')}
				<!-- 読み取りの失敗で使い切ったときだけ急ぎで読み上げる。切り替わりで読み上げ直すよう作り直す。 -->
				{#key stage === 'error'}
					<OcrQuotaCard
						quota={outOfQuota ? null : (quotas?.[currentQuotaIndex] ?? null)}
						exhausted={outOfQuota}
						low={quotaLow}
						othersRemaining={!!quotas?.some(
							(quota, index) => index > currentQuotaIndex && quota.remainingPercent > 0
						)}
						showTopup={topupAvailable}
						urgent={stage === 'error'}
					/>
				{/key}
			{/if}
			<!-- 無効なら、使い切りより先にそれを知らせる。 -->
			{#if offerManualEntry && ocrDisabled}
				<p class="text-sm font-bold">{m.error_ocr_disabled()}</p>
			{/if}
			<!-- 並べる操作は、画面の下端のボタンと同じく、控えめな操作を左・主な操作を右に横並びにする。
			     買い足しは状態によらず左に置き、右は「写真を選ぶ」か、使い切った後 (写真を選んでも読み取れない) の
			     「数字で入力する」にする (docs/ocr.md)。 -->
			{#if offerManualEntry}
				{@render exhaustedActions()}
			{:else if topupAvailable && stage === 'pick'}
				<ButtonRow>
					{@render topupButton()}
					<Button type="button" onclick={pickPhoto}>
						{m.photo_page_pick_photo_button()}
					</Button>
				</ButtonRow>
			{:else if topupAvailable && (stage === 'confirm' || (stage === 'error' && !readingBlocked))}
				<!-- 枠の残りを出す段階だけに置く。読み取れない失敗では、下の「数字で入力する」の左に並べる。 -->
				{@render topupButton()}
			{/if}
			{#if stage === 'pick'}
				{#if !offerManualEntry && !topupAvailable}
					<Button type="button" onclick={pickPhoto}>{m.photo_page_pick_photo_button()}</Button>
				{/if}
			{:else if stage === 'confirm'}
				{#if !offerManualEntry}
					<p class="text-sm text-muted-foreground">
						{m.photo_page_confirm_note({ button: m.photo_page_read_button() })}
					</p>
				{/if}
			{:else if stage === 'reading'}
				<LoadingIndicator class="py-4" label={m.photo_page_reading()} />
				<p class="text-sm text-muted-foreground">{m.photo_page_reading_estimate()}</p>
			{:else if stage === 'error'}
				<!-- 使い切りの失敗は、上の枠が同じことを知らせるので重ねない。 -->
				{#if errorCode !== 'ocr_budget_exhausted'}
					<LoadError message={errorText} />
				{/if}
				{#if readingBlocked}
					{@render exhaustedActions()}
				{:else}
					{@render fallbackLinks(true)}
				{/if}
			{:else if reading}
				<OcrConfidence
					confidence={reading.confidence}
					class="w-full rounded-md px-4 py-3 text-sm"
				/>
				<!-- 「低」なら、帯の色だけでなく文でも、写真と見比べるよう促す (docs/ocr.md)。 -->
				{#if confidenceLevel(reading.confidence) === 'low'}
					<p class="text-sm font-bold text-destructive">{m.photo_page_review_note_low()}</p>
				{:else}
					<p class="text-sm text-muted-foreground">{m.photo_page_review_note()}</p>
				{/if}
				<!-- 読み取り結果の下にそのままフォームを置き、このページで登録する (docs/ocr.md)。
				     送信ボタンは下端のボタンの帯にあり、`form` 属性でこのフォームを指す。 -->
				<RecordForm
					id="photo-record-form"
					idPrefix="ocr"
					onsubmit={saveReading}
					onsubmitted={() => {
						guard.allow();
						leave();
					}}
					bind:submitting
					bind:measuredOnDate={
						() => resultDate,
						(v) => {
							resultDate = v;
							markEdited();
						}
					}
					bind:time={
						() => resultTime,
						(v) => {
							resultTime = v;
							markEdited();
						}
					}
					bind:systolic={
						() => resultSystolic,
						(v) => {
							resultSystolic = v;
							markEdited();
						}
					}
					bind:diastolic={
						() => resultDiastolic,
						(v) => {
							resultDiastolic = v;
							markEdited();
						}
					}
					bind:pulse={
						() => resultPulse,
						(v) => {
							resultPulse = v;
							markEdited();
						}
					}
					bind:memo={
						() => resultMemo,
						(v) => {
							resultMemo = v;
							markEdited();
						}
					}
				/>
			{:else if memoRows}
				<ImportRows
					bind:rows={memoRows}
					periods={periodThresholds}
					thisYear={currentYearIn(userTimeZone())}
					statusOf={memoRowStatus}
					noteOf={memoRowNote}
					noneKeptNote={m.photo_page_memo_none_kept_note()}
					onedit={(row) => memoSheet?.show(row)}
					onkeepchange={markEdited}
				>
					{#snippet beforeList()}
						{#if memoYear !== null}
							<!-- ADR: 年の数が多く、モバイルでは OS 標準のピッカーの方が選びやすいので、ネイティブの select にする
							     (設定のタイムゾーンと同じ)。 -->
							<div class="flex flex-col gap-1.5">
								<label for="memo-year" class="text-sm font-bold"
									>{m.photo_page_memo_year_label()}</label
								>
								<select
									id="memo-year"
									class={[FIELD_SELECT_CLASS, 'w-32']}
									aria-describedby="memo-year-description"
									bind:value={() => memoYear, (year) => changeMemoYear(Number(year))}
								>
									{#each memoYearOptions as year (year)}
										<option value={year}>{m.photo_page_memo_year_option({ year })}</option>
									{/each}
								</select>
								<p id="memo-year-description" class="text-sm text-muted-foreground">
									{m.photo_page_memo_year_description()}
								</p>
							</div>
						{/if}
					{/snippet}
				</ImportRows>
			{:else}
				<p class="text-sm">{m.photo_page_no_values()}</p>
				{#if emptyResultNote}
					<p class="text-sm text-muted-foreground">{emptyResultNote}</p>
				{/if}
				{@render fallbackLinks(false)}
			{/if}
		</div>

		<!-- 読み取り中は押せる操作が無いため、下端のボタン列ごと出さない。
		     使い切った後は、読み取っても選び直しても読み取れないので、読み取る前の段階でも出さない。 -->
		{#if (stage === 'confirm' && !offerManualEntry) || stage === 'result'}
			<ScreenFooter>
				{#if stage === 'confirm'}
					<!-- 誤って選んだ写真で読み取りの枠を使わないよう、押すまで読み取らない (docs/ocr.md)。 -->
					{@render retakeButton(false)}
					<Button type="button" class="flex-1 shadow-soft dark:shadow-none" onclick={requestOcr}>
						{m.photo_page_read_button()}
					</Button>
				{:else if reading}
					{@render retakeButton(false)}
					<LoadingButton
						type="submit"
						form="photo-record-form"
						class="flex-1 shadow-soft dark:shadow-none"
						loading={submitting}
					>
						{recordFormSubmitLabel('create')}
					</LoadingButton>
				{:else if memoRows}
					{@render retakeButton(false)}
					<Button
						type="button"
						class="flex-1 shadow-soft dark:shadow-none"
						onclick={pushPreviewStep}
						disabled={memoRange === null || carryingOverRows === memoRows}
					>
						{m.common_review_button()}
					</Button>
				{:else}
					{@render retakeButton(true)}
				{/if}
			</ScreenFooter>
		{/if}
	{/if}
</div>

<!-- 読み取りの段階が変わっても外さない (開いたまま外すと、積んだ履歴だけが残るため)。 -->
{#if photoUrl}
	<PhotoLightbox bind:this={lightbox} src={photoUrl} alt={m.photo_page_preview_alt()} />
{/if}

<LeaveConfirmDialog {guard} />

<ImportRowSheet
	bind:this={memoSheet}
	idPrefix="memo-row"
	statusOf={memoRowStatus}
	{photoUrl}
	onshowphoto={(from) => lightbox?.show(from)}
	onapplied={memoRowApplied}
/>

<NoticeDialog bind:this={errorDialog} />

<SameRecordDialog bind:this={sameRecordDialog} />

<TopupConfirmDialog bind:this={topupDialog} onpurchased={afterTopup} />

<OcrConsentDialog bind:this={consentDialog} onagreed={consentGiven} ondeclined={enterManually} />
