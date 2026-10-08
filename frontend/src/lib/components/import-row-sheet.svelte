<script lang="ts" generics="T extends ImportRow">
	import { applyEditedRow, type ImportRow, type ImportRowStatus } from '$lib/import';
	import * as m from '$lib/paraglide/messages.js';
	import type { CreateRecordRequest } from '$lib/records';
	import RecordFormSheet from '$lib/components/record-form-sheet.svelte';
	import PhotoPreview from '$lib/components/photo-preview.svelte';

	/** 取り込みの記録を直すシート (docs/import-export.md)。CSV の取り込みと手書きメモの読み取りで共通。
	 * 一覧から開くので削除は出さない (モード「編集・削除なし」)。開いた時点から入力エラーを出す。
	 * `show(row)` で開き、「更新する」で行に書き戻す (`applyEditedRow`)。 */
	let {
		idPrefix,
		statusOf,
		photoUrl,
		onshowphoto,
		onapplied
	}: {
		/** 欄の id の接頭辞。 */
		idPrefix: string;
		/** 行の状態。書き戻す前に「要確認」だった行を「修正済み」にするのに使う (一覧の `statusOf` と同じもの)。 */
		statusOf: (row: T) => ImportRowStatus;
		/** 読み取った写真。渡したときだけ見出しの下に写真の枠を置く (写真で記録。見比べながら直せるように)。
		 * ボタンではなく写真そのものを見せる。ボタンだと入力欄や「更新する」に目が行き、写真を見られることに気づきにくいため。 */
		photoUrl?: string | null;
		/** 写真を全画面で出す。広がる動きの起点の要素を渡す。 */
		onshowphoto?: (from: HTMLElement | undefined) => void;
		/** 行に書き戻した後に呼ぶ。 */
		onapplied?: (row: T) => void;
	} = $props();

	let sheet = $state<ReturnType<typeof RecordFormSheet> | null>(null);
	/** シートで直している行。 */
	let editingRow: T | null = null;
	/** 見出しの下に出す断り書き。開いた時点で決める (書き戻すと `timeIsPlaceholder` が外れ、
	 * 閉じるアニメーションの途中で消えて見えるため、行から導出しない)。 */
	let notice = $state<string | undefined>(undefined);

	/** 行の値を直すシートを開く。仮の時刻を振ったことは、ここでだけ伝える (docs/ocr.md)。一覧では
	 * 時刻の代わりに「朝」「夜」と出すだけにする (全行に断り書きが並ぶのを避けるため)。 */
	export function show(row: T) {
		editingRow = row;
		notice = row.timeIsPlaceholder ? m.import_rows_placeholder_time_notice() : undefined;
		sheet?.show({
			measuredOnDate: row.measuredOnDate,
			time: row.time,
			systolic: row.systolic,
			diastolic: row.diastolic,
			pulse: row.pulse,
			memo: row.memo
		});
	}

	/** シートで直した値を行に書き戻す。検証はシートが済ませているので、ここでは失敗しない。 */
	function apply(record: CreateRecordRequest): boolean {
		const row = editingRow;
		if (!row) return false;
		const needsReview = statusOf(row) !== 'ok';
		applyEditedRow(row, record);
		if (needsReview) row.fixed = true;
		onapplied?.(row);
		return true;
	}
</script>

<RecordFormSheet bind:this={sheet} mode="edit" {idPrefix} {notice} errorsOnOpen onsubmit={apply}>
	{#snippet afterHeader()}
		{#if photoUrl && onshowphoto}
			<PhotoPreview
				class="mb-5 h-36"
				startZoomed
				src={photoUrl}
				alt={m.photo_page_preview_alt()}
				onopen={onshowphoto}
			/>
		{/if}
	{/snippet}
</RecordFormSheet>
