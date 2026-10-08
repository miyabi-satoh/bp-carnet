<script lang="ts">
	import { invalidateAll } from '$app/navigation';
	import { scheduleAccountDeletion } from '$lib/auth';
	import DialogAlertTitleRow from '$lib/components/dialog-alert-title-row.svelte';
	import DialogFormFooter from '$lib/components/dialog-form-footer.svelte';
	import DialogResult from '$lib/components/dialog-result.svelte';
	import LoadingButton from '$lib/components/loading-button.svelte';
	import NoticeDialog from '$lib/components/notice-dialog.svelte';
	import * as Dialog from '$lib/components/ui/dialog';
	import * as Field from '$lib/components/ui/field';
	import { Input } from '$lib/components/ui/input';
	import { busyCloseGuard, DESTRUCTIVE_ACTION_CLASS } from '$lib/dialog';
	import * as m from '$lib/paraglide/messages.js';
	import RequiredMark from '$lib/components/required-mark.svelte';

	/** アカウント削除の確認モーダル。ヘッダーのユーザーメニューと設定画面の両方から開くため、
	 * `logout-dialog.svelte` と同じ `bind:this` + `show()` のパターンにする。 */
	let open = $state(false);
	let confirmation = $state('');
	let loading = $state(false);
	const closeGuard = busyCloseGuard(() => loading);
	/** 予約できたら同じモーダルの中身を結果に差し替える (削除は一度で完了する操作ではなく、
	 * 「いつ消えるか」「どうすれば戻せるか」を読んでもらう必要があるため)。 */
	let scheduled = $state(false);
	let errorDialog = $state<ReturnType<typeof NoticeDialog> | null>(null);

	let canSubmit = $derived(confirmation.trim() === m.delete_account_dialog_confirm_word());

	export function show() {
		confirmation = '';
		scheduled = false;
		open = true;
	}

	async function submit(event: SubmitEvent) {
		event.preventDefault();
		if (loading || !canSubmit) return;

		loading = true;
		const result = await scheduleAccountDeletion();
		loading = false;
		if (!result.ok) {
			errorDialog?.show(m.delete_account_dialog_error_title(), result.message);
			return;
		}
		scheduled = true;
		// 設定画面に予定日を出すため、ログイン中のユーザー (`/auth/me`) を読み直す。
		await invalidateAll();
	}
</script>

<Dialog.Root bind:open>
	<!-- 戻せない操作の確認: × を出さず、警告アイコンと赤い実行ボタン。 -->
	<Dialog.Content {...closeGuard}>
		{#if scheduled}
			<DialogResult
				title={m.delete_account_dialog_scheduled_title()}
				description={m.delete_account_dialog_scheduled_description()}
			/>
		{:else}
			<Dialog.Header>
				<DialogAlertTitleRow>
					<Dialog.Title>{m.delete_account_dialog_title()}</Dialog.Title>
				</DialogAlertTitleRow>
				<Dialog.Description>{m.delete_account_dialog_description()}</Dialog.Description>
			</Dialog.Header>
			<form onsubmit={submit}>
				<Field.Field>
					<Field.FieldLabel for="delete-account-confirmation">
						{m.delete_account_dialog_confirm_label()}<RequiredMark />
					</Field.FieldLabel>
					<Input
						id="delete-account-confirmation"
						class="border-destructive"
						required
						bind:value={confirmation}
						autocomplete="off"
						autocapitalize="off"
					/>
				</Field.Field>
				<DialogFormFooter {loading} oncancel={() => (open = false)}>
					<LoadingButton
						type="submit"
						variant="destructive"
						class={DESTRUCTIVE_ACTION_CLASS}
						disabled={!canSubmit}
						{loading}
					>
						{m.delete_account_dialog_submit_button()}
					</LoadingButton>
				</DialogFormFooter>
			</form>
		{/if}
	</Dialog.Content>
</Dialog.Root>

<NoticeDialog bind:this={errorDialog} />
