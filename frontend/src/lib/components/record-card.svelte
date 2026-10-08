<script lang="ts">
	import { userTimeZone } from '$lib/auth';
	import * as m from '$lib/paraglide/messages.js';
	import { currentYearIn, formatLocalDateTime, formatRecordCardDateTime } from '$lib/period';
	import type { BpRecord } from '$lib/records';
	import { Button } from '$lib/components/ui/button';
	import RecordValues from '$lib/components/record-values.svelte';
	import PencilIcon from '@lucide/svelte/icons/pencil';
	import Trash2Icon from '@lucide/svelte/icons/trash-2';

	/** 履歴1件分のカード。
	 *
	 * ADR: モバイル幅ではテーブルが横にはみ出して操作列が画面外に出るため、
	 * カードのリストにする。
	 *
	 * `onedit`・`ondelete` を渡さなければ、直す・消すボタンを出さない (管理者の閲覧は読み取り専用)。 */
	let {
		record,
		disabled = false,
		timeZone,
		onedit,
		ondelete
	}: {
		record: BpRecord;
		disabled?: boolean;
		/** 今年かどうかを決めるタイムゾーン。省略時はログイン中ユーザーの個人設定 (管理者の閲覧では
		 * 対象ユーザーのものを渡す)。 */
		timeZone?: string;
		onedit?: (record: BpRecord) => void;
		ondelete?: (record: BpRecord) => void;
	} = $props();

	const measuredAt = $derived(formatLocalDateTime(record.localMeasuredAt));
</script>

<div class="flex items-center justify-between gap-3 rounded-md raised bg-card p-3 sm:p-4">
	<div class="min-w-0">
		<RecordValues
			dateTime={formatRecordCardDateTime(record, currentYearIn(timeZone ?? userTimeZone()))}
			systolic={record.systolic}
			diastolic={record.diastolic}
			pulse={record.pulse}
			memo={record.memo}
		/>
	</div>
	<div class="flex shrink-0 gap-1">
		{#if onedit}
			<Button
				type="button"
				variant="ghost"
				size="icon"
				class="rounded-full"
				{disabled}
				aria-label={m.record_card_edit_label({ measuredAt })}
				onclick={() => onedit(record)}
			>
				<PencilIcon />
			</Button>
		{/if}
		{#if ondelete}
			<Button
				type="button"
				variant="ghost"
				size="icon"
				class="rounded-full"
				{disabled}
				aria-label={m.record_card_delete_label({ measuredAt })}
				onclick={() => ondelete(record)}
			>
				<Trash2Icon />
			</Button>
		{/if}
	</div>
</div>
