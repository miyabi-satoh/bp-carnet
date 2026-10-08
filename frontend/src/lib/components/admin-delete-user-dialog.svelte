<script lang="ts">
	import { scheduleUserDeletion, type AdminUser } from '$lib/admin';
	import { userDisplayName, userTimeZone } from '$lib/auth';
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
	import { formatFutureDateLabel } from '$lib/period';
	import RequiredMark from '$lib/components/required-mark.svelte';

	/** ユーザー削除の確認モーダル。一覧のカードから `bind:this` + `show()` で開く
	 * (`delete-account-dialog.svelte` と同じパターン)。 */
	let { onscheduled }: { onscheduled: () => void } = $props();

	let open = $state(false);
	let user = $state<AdminUser | null>(null);
	let confirmation = $state('');
	let loading = $state(false);
	const closeGuard = busyCloseGuard(() => loading);
	/** 予約できたら同じモーダルの中身を結果に差し替える (いつ消えるか・どうすれば戻せるかを
	 * 読んでもらう必要があるため)。 */
	let scheduledAt = $state<string | null>(null);
	let errorDialog = $state<ReturnType<typeof NoticeDialog> | null>(null);

	/** ADR: 確認文字列は本人削除の「削除する」ではなく対象のユーザーIDにする。他人のアカウントを
	 * 消す操作なので、隣のカードのボタンを誤って押しても入力段階で相手違いに気づけるようにする。 */
	let canSubmit = $derived(user !== null && confirmation.trim() === user.username);

	export function show(target: AdminUser) {
		user = target;
		confirmation = '';
		scheduledAt = null;
		open = true;
	}

	async function submit(event: SubmitEvent) {
		event.preventDefault();
		if (loading || !user || !canSubmit) return;

		loading = true;
		const result = await scheduleUserDeletion(user.id);
		loading = false;
		if (!result.ok) {
			errorDialog?.show(m.admin_users_delete_error_title(), result.message);
			return;
		}
		scheduledAt = result.deletionScheduledAt;
		onscheduled();
	}
</script>

<Dialog.Root bind:open>
	<!-- 戻せない操作の確認: × を出さず、警告アイコンと赤い実行ボタン。 -->
	<Dialog.Content {...closeGuard}>
		{#if scheduledAt !== null}
			<DialogResult
				title={m.admin_users_delete_scheduled_title()}
				description={m.admin_users_delete_scheduled_description({
					date: formatFutureDateLabel(new Date(scheduledAt), userTimeZone())
				})}
			/>
		{:else if user}
			<Dialog.Header>
				<DialogAlertTitleRow>
					<Dialog.Title>
						{m.admin_users_delete_dialog_title({ name: userDisplayName(user) })}
					</Dialog.Title>
				</DialogAlertTitleRow>
				<Dialog.Description>{m.admin_users_delete_dialog_description()}</Dialog.Description>
			</Dialog.Header>
			<form onsubmit={submit}>
				<Field.Field>
					<Field.FieldLabel for="delete-user-confirmation">
						{m.admin_users_delete_confirm_label({ username: user.username })}<RequiredMark />
					</Field.FieldLabel>
					<Input
						id="delete-user-confirmation"
						class="border-destructive"
						required
						autocomplete="off"
						autocapitalize="off"
						bind:value={confirmation}
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
						{m.admin_users_delete_submit_button()}
					</LoadingButton>
				</DialogFormFooter>
			</form>
		{/if}
	</Dialog.Content>
</Dialog.Root>

<NoticeDialog bind:this={errorDialog} />
