<script lang="ts">
	import { resetUserPassword, type AdminUser } from '$lib/admin';
	import { userDisplayName } from '$lib/auth';
	import DialogFormHeader from '$lib/components/dialog-form-header.svelte';
	import DialogFormFooter from '$lib/components/dialog-form-footer.svelte';
	import DialogResult from '$lib/components/dialog-result.svelte';
	import LoadingButton from '$lib/components/loading-button.svelte';
	import LoadingIndicator from '$lib/components/loading-indicator.svelte';
	import LoadError from '$lib/components/load-error.svelte';
	import NewPasswordFields from '$lib/components/new-password-fields.svelte';
	import NoticeDialog from '$lib/components/notice-dialog.svelte';
	import * as Dialog from '$lib/components/ui/dialog';
	import * as Field from '$lib/components/ui/field';
	import { busyCloseGuard, focusFirstFieldOnOpen } from '$lib/dialog';
	import { NewPasswordState } from '$lib/new-password.svelte';
	import * as m from '$lib/paraglide/messages.js';

	/** パスワード再設定のモーダル。一覧のカードから `bind:this` + `show()` で開く。
	 * カードごとにモーダルを置くとユーザーの数だけ状態を持つことになるため、画面に1つだけ
	 * 置いて対象を差し替える。 */
	let { onreset }: { onreset: () => void } = $props();

	let open = $state(false);
	let user = $state<AdminUser | null>(null);
	let loading = $state(false);
	const closeGuard = busyCloseGuard(() => loading);
	let content = $state<HTMLElement | null>(null);
	const focusOnOpen = focusFirstFieldOnOpen(() => content);
	/** 再設定できたら中身を結果に差し替える (本人に伝える操作が残っているため、閉じる前に
	 * 一度読んでもらう)。 */
	let done = $state(false);
	let errorDialog = $state<ReturnType<typeof NoticeDialog> | null>(null);

	const passwordForm = new NewPasswordState();

	export function show(target: AdminUser) {
		user = target;
		passwordForm.reset();
		done = false;
		open = true;
		passwordForm.loadPolicy();
	}

	async function submit(event: SubmitEvent) {
		event.preventDefault();
		if (loading || !user || !passwordForm.validate()) return;

		loading = true;
		const result = await resetUserPassword(user.id, passwordForm.password);
		loading = false;
		if (!result.ok) {
			// 閉じると入力が消えるため開いたままにする (通信エラー等でやり直せるように)。
			errorDialog?.show(m.admin_users_reset_password_error_title(), result.message);
			return;
		}
		done = true;
		onreset();
	}
</script>

<Dialog.Root bind:open>
	<Dialog.Content bind:ref={content} {...closeGuard} onOpenAutoFocus={focusOnOpen}>
		{#if done}
			<DialogResult
				title={m.admin_users_reset_password_done_title()}
				description={m.admin_users_reset_password_done_description()}
			/>
		{:else if user}
			<DialogFormHeader
				title={m.admin_users_reset_password_dialog_title()}
				description={m.admin_users_reset_password_dialog_description({
					name: userDisplayName(user)
				})}
				closeDisabled={loading}
			/>
			{#if passwordForm.loadErrorMessage}
				<LoadError message={passwordForm.loadErrorMessage} onretry={passwordForm.loadPolicy} />
			{:else if passwordForm.policy === null}
				<LoadingIndicator />
			{:else}
				<form onsubmit={submit}>
					<Field.FieldGroup class="gap-3.5">
						<NewPasswordFields
							policy={passwordForm.policy}
							passwordLabel={m.common_new_password_label()}
							confirmLabel={m.common_new_password_confirm_label()}
							idPrefix="reset-password"
							bind:password={passwordForm.password}
							bind:confirmation={passwordForm.confirmation}
							submitted={passwordForm.submitted}
						/>
					</Field.FieldGroup>
					<!-- 押す前に、対象の利用者が使っている画面が閉じられることを知らせる。 -->
					<p class="mt-3 text-xs text-muted-foreground">
						{m.admin_users_reset_password_notice()}
					</p>
					<DialogFormFooter {loading} oncancel={() => (open = false)}>
						<LoadingButton type="submit" {loading}>
							{m.admin_users_reset_password_submit_button()}
						</LoadingButton>
					</DialogFormFooter>
				</form>
			{/if}
		{/if}
	</Dialog.Content>
</Dialog.Root>

<NoticeDialog bind:this={errorDialog} />
