<script lang="ts">
	import * as m from '$lib/paraglide/messages.js';
	import { busyCloseGuard, DESTRUCTIVE_ACTION_CLASS } from '$lib/dialog';
	import { formatLocalDateTime } from '$lib/period';
	import { deleteRecord, type BpRecord } from '$lib/records';
	import * as AlertDialog from '$lib/components/ui/alert-dialog';
	import DialogAlertTitleRow from '$lib/components/dialog-alert-title-row.svelte';
	import LoadingButton from '$lib/components/loading-button.svelte';
	import NoticeDialog from '$lib/components/notice-dialog.svelte';

	/** 一覧のカードのゴミ箱から開く、記録の削除の確認。
	 * 開く操作は `record-form-dialog.svelte` と同じ `bind:this` + `show()` にそろえる。 */
	let {
		/** 記録が変わったとき (一覧の再取得は呼び出し側の責務)。消せたとき (`deleted` が true) と、
		 * ほかで変わった・消されたと分かったとき。 */
		onchanged
	}: {
		onchanged: (deleted: boolean) => void;
	} = $props();

	let open = $state(false);
	let deleting = $state(false);
	/** 消せて閉じたか。開いたボタンは一覧ごと消えるので、閉じたときにそこへフォーカスを戻さない。 */
	let deleted = false;
	/** 消す記録。開いた時点の一覧の値で、確認文にもこれを出す。 */
	let target = $state<BpRecord | null>(null);
	let errorDialog = $state<ReturnType<typeof NoticeDialog> | null>(null);

	const closeGuard = busyCloseGuard(() => deleting);

	export function show(record: BpRecord) {
		target = record;
		deleted = false;
		open = true;
	}

	async function handleDelete() {
		if (!target || deleting) return;
		deleting = true;
		const result = await deleteRecord(target);
		deleting = false;
		if (result.ok) {
			deleted = true;
			open = false;
			onchanged(true);
			return;
		}
		const title = m.record_delete_error_title();
		// ほかで変わった・消された: 閉じて一覧を最新にし、消すかどうかは最新の値を見て決め直してもらう。
		const stale =
			result.code === 'not_found'
				? m.record_deleted_elsewhere()
				: result.code === 'record_conflict'
					? m.record_delete_changed_elsewhere()
					: undefined;
		if (stale) {
			open = false;
			errorDialog?.show(title, stale);
			onchanged(false);
			return;
		}
		// ほかの失敗は、開いたままその場でやり直せるようにする。
		errorDialog?.show(title, result.message);
	}
</script>

<!-- 戻せない操作の確認: × を出さず、警告アイコンと赤い実行ボタン。 -->
<AlertDialog.Root bind:open>
	<AlertDialog.Content
		{...closeGuard}
		onCloseAutoFocus={(e) => {
			if (deleted) e.preventDefault();
		}}
	>
		<AlertDialog.Header>
			<DialogAlertTitleRow>
				<AlertDialog.Title>{m.record_delete_confirm_title()}</AlertDialog.Title>
			</DialogAlertTitleRow>
			<AlertDialog.Description>
				{#if target}
					{m.record_delete_confirm_description({
						measuredAt: formatLocalDateTime(target.localMeasuredAt),
						systolic: target.systolic,
						diastolic: target.diastolic
					})}
				{/if}
			</AlertDialog.Description>
		</AlertDialog.Header>
		<AlertDialog.Footer>
			<AlertDialog.Cancel disabled={deleting}>{m.common_cancel_button()}</AlertDialog.Cancel>
			<LoadingButton
				variant="destructive"
				class={DESTRUCTIVE_ACTION_CLASS}
				loading={deleting}
				onclick={handleDelete}
			>
				{m.common_delete_button()}
			</LoadingButton>
		</AlertDialog.Footer>
	</AlertDialog.Content>
</AlertDialog.Root>

<NoticeDialog bind:this={errorDialog} />
