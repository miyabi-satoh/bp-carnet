<script lang="ts">
	import * as m from '$lib/paraglide/messages.js';
	import { userTimeZone } from '$lib/auth';
	import { toLocalDateTime } from '$lib/period';
	import type { ApiFailure } from '$lib/api/errors';
	import { splitMeasuredAt, type RecordFormMode, type RecordFormValues } from '$lib/record-form';
	import {
		fetchRecord,
		saveRecord,
		type CreateRecordRequest,
		type RecordResponse
	} from '$lib/records';
	import RecordFormSheet from '$lib/components/record-form-sheet.svelte';
	import NoticeDialog from '$lib/components/notice-dialog.svelte';

	/** 記録の登録・編集フォーム。シートの作りと命令的に開く理由は `record-form-sheet.svelte` にあり、
	 * ここは API の呼び出しと通信エラーの表示 (エラーダイアログ) だけを受け持つ。 */
	let {
		/** 記録が変わったとき (一覧の再取得は呼び出し側の責務)。登録・更新が成功したときと、
		 * ほかで変わった・消されたと分かったとき。登録・更新なら `savedOn` にその記録の日
		 * (YYYY-MM-DD) を渡す。 */
		onchanged
	}: {
		onchanged: (savedOn?: string) => void;
	} = $props();

	let sheet = $state<ReturnType<typeof RecordFormSheet> | null>(null);
	let errorDialog = $state<ReturnType<typeof NoticeDialog> | null>(null);

	/** 編集中の記録。開いた時点のスナップショットで、一覧からは導出しない。導出にすると、
	 * 期間フィルタを切り替えて対象が表示範囲外になっただけで編集状態が崩れ、未保存の入力が
	 * 消えてしまう。`null` なら新規登録モード。 */
	let editing = $state<RecordResponse | null>(null);
	let mode = $state<RecordFormMode>('create');
	/** 登録・更新した記録の日。シートが閉じたときに `onchanged` へ渡す。 */
	let savedOn: string | undefined;

	/** ほかで記録が変わって最新を出し直したときの案内。開き直すと消す。 */
	let notice = $state<string | undefined>(undefined);

	function formValues(record: RecordResponse): RecordFormValues {
		return {
			...splitMeasuredAt(record.localMeasuredAt),
			systolic: String(record.systolic),
			diastolic: String(record.diastolic),
			pulse: String(record.pulse ?? ''),
			memo: record.memo
		};
	}

	/** 指定したレコードの編集モードで開く。`null` なら新規登録モード。 */
	export function show(target: RecordResponse | null) {
		editing = target;
		mode = target ? 'edit' : 'create';
		notice = undefined;
		savedOn = undefined;
		sheet?.show(
			target
				? formValues(target)
				: {
						...splitMeasuredAt(toLocalDateTime(new Date(), userTimeZone())),
						systolic: '',
						diastolic: '',
						pulse: '',
						memo: ''
					}
		);
	}

	/** 直すのが、ほかで変わった・消された記録で止められたときの扱い。
	 * 扱い終えたら、シートを閉じるか (`true`) を返す。ほかの失敗なら `undefined`。 */
	async function handleStale(
		target: RecordResponse,
		result: ApiFailure,
		errorTitle: string
	): Promise<boolean | undefined> {
		if (result.code === 'not_found') {
			errorDialog?.show(errorTitle, m.record_deleted_elsewhere());
			return true;
		}
		if (result.code !== 'record_conflict') return undefined;

		const latest = await fetchRecord(target.id);
		// 取り直す間に消されていた。
		if (!latest.ok && latest.code === 'not_found') return handleStale(target, latest, errorTitle);
		if (!latest.ok) {
			errorDialog?.show(errorTitle, latest.message);
			return false;
		}
		// 閉じた・別の記録を開き直した後に届いた結果は捨てる。
		if (editing !== target) return false;
		editing = latest.data;
		notice = m.record_form_dialog_changed_elsewhere();
		sheet?.refresh(formValues(latest.data));
		// 一覧も最新にする。古い版番号のまま開き直して、また止められないように。
		onchanged();
		return false;
	}

	async function submit(record: CreateRecordRequest): Promise<boolean> {
		const target = editing;
		const result = await saveRecord(target, record);
		if (result.ok) {
			savedOn = record.localMeasuredAt.slice(0, 10);
			return true;
		}
		const errorTitle = target
			? m.record_form_dialog_update_error_title()
			: m.record_form_dialog_create_error_title();
		const stale = target ? await handleStale(target, result, errorTitle) : undefined;
		if (stale !== undefined) return stale;
		errorDialog?.show(errorTitle, result.message);
		return false;
	}
</script>

<RecordFormSheet
	bind:this={sheet}
	{mode}
	{notice}
	onsubmit={submit}
	onfinished={() => onchanged(savedOn)}
/>

<NoticeDialog bind:this={errorDialog} />
