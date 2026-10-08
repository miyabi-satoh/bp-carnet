<script lang="ts">
	import { onMount, tick } from 'svelte';
	import { afterNavigate, beforeNavigate, goto, replaceState } from '$app/navigation';
	import { resolve } from '$app/paths';
	import { page } from '$app/state';
	import * as m from '$lib/paraglide/messages.js';
	import { userTimeZone } from '$lib/auth';
	import {
		formatDateOnly,
		localToday,
		parseLocalDate,
		periodCovering,
		periodFor,
		periodToShow
	} from '$lib/period';
	import { takeImportResult } from '$lib/import-step';
	import { takeManualEntryRequest } from '$lib/record-form';
	import { fetchLatestRecordDate, fetchRecordsWithSummary, type BpRecord } from '$lib/records';
	import { RecordsViewState } from '$lib/records-view.svelte';
	import { LatestRequest } from '$lib/latest-request';
	import { buildReportHref, saveHomeViewBeforeReport, takeHomeViewBeforeReport } from '$lib/report';
	import { Button } from '$lib/components/ui/button';
	import NoticeDialog from '$lib/components/notice-dialog.svelte';
	import RecordsOverview from '$lib/components/records-overview.svelte';
	import RecordFormDialog from '$lib/components/record-form-dialog.svelte';
	import RecordDeleteDialog from '$lib/components/record-delete-dialog.svelte';
	import PeriodNav from '$lib/components/period-nav.svelte';
	import CameraIcon from '@lucide/svelte/icons/camera';
	import PlusIcon from '@lucide/svelte/icons/plus';
	import PrinterIcon from '@lucide/svelte/icons/printer';
	import { fetchOcrStatus, PHOTO_FILE_ACCEPT, photoFileHandoff } from '$lib/ocr';
	import { openFilePicker } from '$lib/utils';

	/** 記録の一覧と集計。初期表示は最新の記録を含む週 (記録が無ければ今週)。 */
	const recordsView = new RecordsViewState(fetchRecordsWithSummary);
	const today = localToday(userTimeZone());
	/** 最新の記録の日。初期表示の週と、期間ナビの「クリア」の戻り先を決める。記録が無い・取得に
	 * 失敗したときは `undefined` (今週にする。最新の日が分からないだけで画面は使えるため)。 */
	let latestDate = $state<Date | undefined>();
	/** 記録が1件も無いと分かっているか。取得に失敗したら分からないので `false`。 */
	let neverRecorded = $state(false);
	/** 期間ナビを出してよいか。最新の日が分かる前に出すと、今週から最新の週へ表示が切り替わる。 */
	let periodReady = $state(false);
	let initialPeriodSet = false;

	const latestDateRequest = new LatestRequest();
	let latestDatePending: Promise<void> = Promise.resolve();

	/** 最新の日を取り直す。続けて呼ばれたときは、遅れて返った古い応答で上書きせず、どの呼び出しも
	 * 最後の呼び出しが終わるまで待つ (待った側が古い `latestDate` で期間を決めないため)。 */
	function loadLatestDate(): Promise<void> {
		const isLatest = latestDateRequest.begin();
		const pending = fetchLatestRecordDate().then((result) => {
			if (!isLatest()) return;
			latestDate = result.ok && result.date !== null ? parseLocalDate(result.date) : undefined;
			neverRecorded = result.ok && result.date === null;
		});
		latestDatePending = pending;
		return pending.then(() => latestDatePending);
	}

	/** 記録を足す・直す・消したあと。登録・更新した記録 (`savedOn` の日) が表示中の期間の外なら
	 * その期間へ移り、一覧の「さらに表示」の後ろなら出す件数を広げる (一覧に見えないと「消えた」と
	 * 思って入れ直してしまうため)。最新の日も変わりうるので、期間ナビの戻り先のために取り直す。 */
	async function reloadAfterChange(savedOn?: string) {
		loadLatestDate();
		const next = savedOn && periodToShow(savedOn, recordsView.from, recordsView.to, today);
		await (next ? recordsView.changePeriod(next.from, next.to) : recordsView.load());
		if (savedOn) recordsView.revealDate(savedOn);
	}

	/** 記録を消した後のフォーカスの行き先。見出しは、取り直しに失敗すると描かれないので、そのときは
	 * 一覧を包む領域へ移す。 */
	let historyHeading = $state<HTMLElement | null>(null);
	let overviewArea = $state<HTMLElement | null>(null);

	/** 記録を消した後は、押したゴミ箱がカードごと消えてフォーカスが行き場を失うので、一覧を
	 * 取り直してから一覧の見出し (無ければ一覧を包む領域) へ移す。 */
	async function reloadAfterDelete(deleted: boolean) {
		await reloadAfterChange();
		if (!deleted) return;
		await tick();
		(historyHeading ?? overviewArea)?.focus();
	}

	/** 印刷用レポートへのリンク。表示中の期間とグラフに表示中の系列を引き継ぐ。 */
	let reportHref = $derived(
		buildReportHref(recordsView.from, recordsView.to, recordsView.visibleSeriesKeys)
	);

	// リンクに限らず、ブラウザの進むで印刷ページへ行くときも控える。
	beforeNavigate(({ to }) => {
		if (to?.route.id !== '/report') return;
		saveHomeViewBeforeReport({
			from: recordsView.from,
			to: recordsView.to,
			series: recordsView.visibleSeriesKeys
		});
	});

	/** `GEMINI_API_KEY` 未設定時は下部バーの「写真で記録」を隠す。最初から出しておき、無効と分かったときだけ
	 * 隠す。取れてから差し込むと、「手動で入力...」が押す直前に縮むため。取れなければ隠さない。 */
	let ocrEnabled = $state(true);
	let photoInput = $state<HTMLInputElement | null>(null);

	/** 写真を選ばせる。選んでいる間にも写真で記録のページのコードを読み込む (開いたときの読み込みが失敗していたとき)。 */
	function openPhotoPicker() {
		photoFileHandoff.preload();
		openFilePicker(photoInput);
	}

	/** 写真をすぐ選ばせ、選んだら写真で記録のページへ渡す (docs/ocr.md)。 */
	function handlePhotoChange(e: Event) {
		const file = (e.currentTarget as HTMLInputElement).files?.[0];
		if (!file) return;
		photoFileHandoff.set(file);
		goto(resolve('/record/photo'));
	}

	/** 登録・編集フォーム (モーダル)。開く操作は `show` で呼び出す。 */
	let recordFormDialog = $state<ReturnType<typeof RecordFormDialog> | null>(null);

	/** 記録の削除の確認。開く操作は `show` で呼び出す。 */
	let recordDeleteDialog = $state<ReturnType<typeof RecordDeleteDialog> | null>(null);

	/** ログイン時の知らせを表示する。 */
	let noticeDialog = $state<ReturnType<typeof NoticeDialog> | null>(null);

	// アカウント削除の予約が、このログインで取り消されたことを知らせる。ID/PW ログインは
	// ログイン画面がこのクエリを付け、Google コールバックはサーバーがこの URL へ返す。
	// 表示後は URL からクエリを取り除く (リロードで再表示しないため、ログイン画面の
	// `oauthError` と同じ作り)。
	afterNavigate(() => {
		if (page.url.searchParams.get('deletionCancelled') !== '1') return;
		noticeDialog?.show(
			m.account_deletion_cancelled_title(),
			m.account_deletion_cancelled_description()
		);
		replaceState(page.url.pathname, {});
	});

	/** 下部アクションバーの「手動で入力...」押下時、新規登録モードでフォームを開く。 */
	function startCreate() {
		recordFormDialog?.show(null);
	}

	/** 履歴の「編集」ボタン押下時、その記録の編集モードでフォームを開く。 */
	function startEdit(record: BpRecord) {
		recordFormDialog?.show(record);
	}

	// 印刷ページから戻ったときは、行く前の期間・系列のまま表示する。
	// 遷移元が分かるここで決めるため、初回の取得も onMount ではなくここで行う。控えは遷移元に
	// よらず消す (印刷ページから別の画面を経て開いたときに、古い状態を出さないため)。
	afterNavigate(async ({ from }) => {
		const restoredView = takeHomeViewBeforeReport();
		if (restoredView && from?.route.id === '/report') {
			// 月タブの初期位置 (`defaultAnchor`) は期間ナビの生成時に決まるため、先に最新の日を得る。
			await loadLatestDate();
			recordsView.from = restoredView.from;
			recordsView.to = restoredView.to;
			recordsView.visibleSeriesKeys = restoredView.series;
			initialPeriodSet = true;
			periodReady = true;
		} else if (!initialPeriodSet) {
			// 同じページの中の遷移では、操作で選んだ期間を初期表示に戻さない。
			// 期間を入れる前に立てると、待つ間の別の遷移が空の期間 (全件) で取得してしまう。
			// 写真で記録から戻ったときは、登録した記録の期間を出す (最新の週に無いと、登録できたか
			// 分からないため)。
			const imported = takeImportResult();
			await loadLatestDate();
			if (imported) {
				const period = periodCovering(imported.from, imported.to, today);
				recordsView.from = period.from;
				recordsView.to = period.to;
			} else {
				const initialPeriod = periodFor('week', latestDate ?? today, today);
				recordsView.from = formatDateOnly(initialPeriod.from);
				recordsView.to = formatDateOnly(initialPeriod.to);
			}
			initialPeriodSet = true;
			periodReady = true;
		}
		recordsView.load();
		// 写真で記録のページの「数字で入力する」から戻ったとき。
		if (takeManualEntryRequest()) startCreate();
	});

	/** 記録が1件も無いときに、下のボタンから記録できると案内する。 */
	let firstRecordHint = $derived(
		ocrEnabled
			? m.home_first_record_hint_with_photo({
					photo: m.home_photo_ocr_button(),
					manual: m.home_add_record_button()
				})
			: m.home_first_record_hint({ manual: m.home_add_record_button() })
	);

	onMount(() => {
		photoFileHandoff.preload();
		fetchOcrStatus().then((status) => (ocrEnabled = status.enabled !== false));
	});
</script>

<!-- 上端の帯 (時計) の裏をスクロールする記録を、期間ナビゲーションと同じぼかしで覆う。期間ナビゲーションを
     top-0 にして帯の分の余白を持たせると、スクロールする前もヘッダーとの間に帯の高さの隙間が空くため、板を分ける。 -->
<div
	aria-hidden="true"
	class="fixed inset-x-0 top-0 z-30 h-safe-top bg-background/95 backdrop-blur supports-[backdrop-filter]:bg-background/80 print:hidden"
></div>

<!-- 期間ナビゲーション。画面上部に常駐させる。 -->
<div
	class="sticky top-safe z-30 border-b bg-background/95 backdrop-blur supports-[backdrop-filter]:bg-background/80 print:hidden"
>
	<div class="mx-auto max-w-2xl p-4">
		{#if periodReady}
			<PeriodNav
				from={recordsView.from}
				to={recordsView.to}
				loading={recordsView.loading}
				defaultAnchor={latestDate}
				onchange={recordsView.changePeriod}
			/>
		{/if}
	</div>
</div>

<div
	bind:this={overviewArea}
	tabindex="-1"
	class="mx-auto flex max-w-2xl flex-col gap-4 p-6 pb-32 outline-none"
>
	<RecordsOverview
		records={recordsView.records}
		summary={recordsView.summary}
		loading={recordsView.loading}
		errorMessage={recordsView.errorMessage}
		onretry={recordsView.load}
		bind:visibleKeys={recordsView.visibleSeriesKeys}
		bind:listLimit={recordsView.listLimit}
		onedit={startEdit}
		ondelete={(record) => recordDeleteDialog?.show(record)}
		emptyMessage={neverRecorded ? firstRecordHint : undefined}
	>
		{#snippet header()}
			<div class="flex items-center justify-between">
				<h2 bind:this={historyHeading} tabindex="-1" class="text-base font-bold outline-none">
					{m.home_history_title()}
				</h2>
				<!-- 主要な操作なので、タップ領域を44px以上にする (requirements 6.1)。 -->
				<Button href={reportHref} variant="outline">
					<PrinterIcon />
					{m.home_report_button()}
				</Button>
			</div>
		{/snippet}
	</RecordsOverview>
</div>

<!-- 下部アクションバー。記録の追加はここだけに集約する。
     一覧の上に重ねるので、コンテンツ側は pb-32 で隠れないようにしている。 -->
<div class="fixed inset-x-0 bottom-0 z-40 border-t bg-card pb-safe-0 print:hidden">
	<div class="mx-auto flex max-w-2xl gap-3 p-3">
		{#if ocrEnabled}
			<Button
				type="button"
				variant="secondary"
				size="lg"
				class="flex-1 border border-primary text-teal-foreground"
				onclick={openPhotoPicker}
			>
				<CameraIcon />
				{m.home_photo_ocr_button()}
			</Button>
		{/if}
		<Button type="button" size="lg" class="flex-1" onclick={() => startCreate()}>
			<PlusIcon />
			{m.home_add_record_button()}
		</Button>
	</div>
</div>

<NoticeDialog bind:this={noticeDialog} />

<input
	bind:this={photoInput}
	type="file"
	accept={PHOTO_FILE_ACCEPT}
	class="hidden"
	onchange={handlePhotoChange}
/>

<RecordFormDialog bind:this={recordFormDialog} onchanged={reloadAfterChange} />
<RecordDeleteDialog bind:this={recordDeleteDialog} onchanged={reloadAfterDelete} />
