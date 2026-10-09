<script lang="ts">
	import type { Snippet } from 'svelte';
	import { busyCloseGuard } from '$lib/dialog';
	import { HistoryOverlay } from '$lib/history-overlay.svelte';
	import { LeaveGuard } from '$lib/leave-guard.svelte';
	import * as m from '$lib/paraglide/messages.js';
	import type { CreateRecordRequest } from '$lib/records';
	import {
		recordFormSubmitLabel,
		recordFormTitle,
		type RecordFormMode,
		type RecordFormValues
	} from '$lib/record-form';
	import * as Dialog from '$lib/components/ui/dialog';
	import * as Field from '$lib/components/ui/field';
	import { Button } from '$lib/components/ui/button';
	import LoadingButton from '$lib/components/loading-button.svelte';
	import DialogFormHeader from '$lib/components/dialog-form-header.svelte';
	import LeaveConfirmDialog from '$lib/components/leave-confirm-dialog.svelte';
	import RecordForm from '$lib/components/record-form.svelte';

	/** 血圧を入れるシート。記録の登録・編集にも、取り込みの行を直すのにも使う。
	 * 見出しとボタンの文言はモードから決まるので、呼び出し側から渡さない。
	 *
	 * ADR: 開閉制御は `bind:open` ではなく `bind:this` + `show()` の命令的APIにする。
	 * 入口ごとに開く時点の初期値が違い、開くのと初期値を渡すのを1回で済ませたいため。
	 *
	 * 開くと `pushState` で履歴を積み、端末の戻る操作でも閉じる。 */
	let {
		mode,
		idPrefix = 'record',
		notice,
		errorsOnOpen = false,
		afterHeader,
		onsubmit,
		onfinished
	}: {
		/** 見出し・ボタンの文言を決める。 */
		mode: RecordFormMode;
		/** 欄の id の接頭辞。 */
		idPrefix?: string;
		/** 見出しの下に出す注意書き。無ければ出さない。 */
		notice?: string;
		/** 開いた時点から入力エラーを出すか (`record-form.svelte`)。 */
		errorsOnOpen?: boolean;
		/** 見出しの下、フォームの上に置くもの (写真で記録の写真の枠など)。 */
		afterHeader?: Snippet;
		/** 検証を通った値で送信する。成功なら `true` を返す (シートを閉じる)。`false` なら
		 * シートは開いたままで、エラーの表示は呼び出し側が受け持つ。 */
		onsubmit: (record: CreateRecordRequest) => boolean | Promise<boolean>;
		/** 送信が成功してシートを閉じた後に呼ぶ。閉じる前にページを移らないよう、
		 * 閉じ終わってから知らせる。 */
		onfinished?: () => void;
	} = $props();

	let measuredOnDate = $state('');
	let time = $state('');
	let systolic = $state('');
	let diastolic = $state('');
	let pulse = $state('');
	let memo = $state('');

	let content = $state<HTMLElement | null>(null);
	/** フォームの外 (ボタンの帯) の送信ボタンから `form` 属性で指す id。 */
	const formId = $derived(`${idPrefix}-form`);
	let submitting = $state(false);

	/** 開くたびに増やし、フォームを作り直す (前に開いたときの入力エラーの表示を残さないため)。 */
	let generation = $state(0);

	/** 開いた時点の値。これと違えば入力・変更したとみなし、閉じる前に確認する。 */
	let initialValues = '';
	function currentValues() {
		return JSON.stringify([measuredOnDate, time, systolic, diastolic, pulse, memo]);
	}

	const guard = new LeaveGuard(
		() => overlay.open && currentValues() !== initialValues,
		() => submitting
	);
	const closeGuard = busyCloseGuard(() => submitting);

	// 端末の戻る操作でシートの履歴から出たら閉じる。閉じられないときは履歴を積み直して開いたままにする:
	// 送信中、確認を開いている (戻る操作はそれだけを閉じる)、入力している (閉じる前に確認する)。
	const overlay = new HistoryOverlay('recordForm', () => {
		const busy = submitting;
		const confirming = guard.dialogOpen;
		const dirty = currentValues() !== initialValues;
		if (!busy && !confirming && !dirty) return true;
		if (!busy && !confirming) guard.confirm(close);
		return false;
	});

	/** シートを閉じ、開いたときに積んだ履歴を戻す。 */
	export function close() {
		overlay.close();
	}

	/** 閉じるボタン・戻るボタン・Escape・外側のクリックで閉じるとき。入力していれば確認する。 */
	function requestClose() {
		guard.confirm(close);
	}

	/** 初期値を入れて開く。閉じるときに値を消さないのは、閉じるアニメーションの最中に
	 * 入力値が消えて見えるため。 */
	export function show(initial: RecordFormValues) {
		generation += 1;
		({ measuredOnDate, time, systolic, diastolic, pulse, memo } = initial);
		initialValues = currentValues();
		overlay.show();
	}

	/** 開いたまま、値を入れ直す (ほかで変わった記録を最新にするとき)。入力途中の値は捨てる。
	 * `show` と違ってフォームを作り直さず履歴も積まない: 送信の処理の最中に呼ばれるため、
	 * 送信中のフォームを消さず、開いたときの履歴をそのまま使う。 */
	export function refresh(values: RecordFormValues) {
		({ measuredOnDate, time, systolic, diastolic, pulse, memo } = values);
		initialValues = currentValues();
	}

	/** 送信が成功したとき。 */
	function finish() {
		close();
		onfinished?.();
	}
</script>

<Dialog.Root
	bind:open={
		() => overlay.open,
		(value) => {
			if (!value) requestClose();
		}
	}
>
	<!-- モバイルは下端から出るシート、sm 以上は中央ダイアログ。
	     Dialog.Content の既定は translate で中央に寄せる指定なので、位置・変形・角丸・幅を
	     まとめて打ち消してから sm: で元に戻している。
	     見出し・中身・ボタンの帯の縦並びにし、中身だけをスクロールさせる。ボタンの帯は
	     スクロールの外に置くので、中身がシートの高さを超えても、いつもシートの下端に見えている。 -->
	<Dialog.Content
		bind:ref={content}
		{...closeGuard}
		onOpenAutoFocus={(e) => {
			// 開いたときはシート自体にフォーカスを送る。既定では先頭の閉じるボタンに当たり、
			// 入力欄へ送るとモバイルで測定日時のピッカーが開いてしまうため。
			// 開いた直後の Shift+Tab は動かない (シート自体はタブ順に入らないため)。
			// Tab を押せば先頭のボタンから順に回る。
			e.preventDefault();
			content?.focus();
		}}
		class="top-auto bottom-0 left-0 flex max-h-[90dvh] max-w-none translate-x-0 translate-y-0 flex-col gap-0 rounded-t-lg rounded-b-none p-0 sm:top-1/2 sm:bottom-auto sm:left-1/2 sm:max-w-md sm:-translate-x-1/2 sm:-translate-y-1/2 sm:rounded-lg"
	>
		<div class="shrink-0 px-5 pt-5">
			<div aria-hidden="true" class="mx-auto mb-4 h-1 w-10 rounded-full bg-border sm:hidden"></div>
			<DialogFormHeader class="mb-5" title={recordFormTitle(mode)} closeDisabled={submitting} />
		</div>
		<div class="min-h-0 flex-1 overflow-y-auto px-5">
			{@render afterHeader?.()}

			{#key generation}
				<RecordForm
					id={formId}
					{idPrefix}
					{notice}
					{errorsOnOpen}
					{onsubmit}
					onsubmitted={finish}
					bind:measuredOnDate
					bind:time
					bind:systolic
					bind:diastolic
					bind:pulse
					bind:memo
					bind:submitting
				/>
			{/key}
		</div>
		<!-- デザインはメモ欄とボタンの間を 24px、ボタンの間を 10px にしている。 -->
		<div class="flex shrink-0 flex-col gap-2 px-5 pt-6 pb-safe-5 sm:pb-5">
			<Field.Field orientation="horizontal" class="gap-2.5 [&>*]:flex-1">
				<Button
					type="button"
					variant="outline"
					class="text-muted-foreground"
					disabled={submitting}
					onclick={requestClose}
				>
					{m.common_back_button()}
				</Button>
				<LoadingButton
					type="submit"
					form={formId}
					class="shadow-soft dark:shadow-none"
					loading={submitting}
				>
					{recordFormSubmitLabel(mode)}
				</LoadingButton>
			</Field.Field>
		</div>
	</Dialog.Content>
</Dialog.Root>

<LeaveConfirmDialog
	{guard}
	title={m.record_form_dialog_discard_title()}
	leaveLabel={m.record_form_dialog_discard_button()}
/>
