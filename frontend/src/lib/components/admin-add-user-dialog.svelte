<script lang="ts">
	import { createUser } from '$lib/admin';
	import DialogFormHeader from '$lib/components/dialog-form-header.svelte';
	import DialogFormFooter from '$lib/components/dialog-form-footer.svelte';
	import DialogResult from '$lib/components/dialog-result.svelte';
	import LoadingButton from '$lib/components/loading-button.svelte';
	import LoadingIndicator from '$lib/components/loading-indicator.svelte';
	import LoadError from '$lib/components/load-error.svelte';
	import NewPasswordFields from '$lib/components/new-password-fields.svelte';
	import NoticeDialog from '$lib/components/notice-dialog.svelte';
	import { Checkbox } from '$lib/components/ui/checkbox';
	import * as Dialog from '$lib/components/ui/dialog';
	import * as Field from '$lib/components/ui/field';
	import UsernameField from '$lib/components/username-field.svelte';
	import { busyCloseGuard, focusFirstFieldOnOpen } from '$lib/dialog';
	import { NewPasswordState } from '$lib/new-password.svelte';
	import * as m from '$lib/paraglide/messages.js';
	import { usernameError } from '$lib/username';

	/** ユーザー追加モーダル。一覧の「追加...」から `bind:this` + `show()` で開く。 */
	let { onadded }: { onadded: () => void } = $props();

	let open = $state(false);
	let username = $state('');
	let asAdmin = $state(false);
	let loading = $state(false);
	const closeGuard = busyCloseGuard(() => loading);
	let content = $state<HTMLElement | null>(null);
	const focusOnOpen = focusFirstFieldOnOpen(() => content);
	/** 作れたら中身を結果に差し替える (パスワードを本人に伝える必要があるため、
	 * 閉じる前に一度読んでもらう)。 */
	let createdUsername = $state<string | null>(null);
	let errorDialog = $state<ReturnType<typeof NoticeDialog> | null>(null);

	const passwordForm = new NewPasswordState();

	export function show() {
		username = '';
		asAdmin = false;
		passwordForm.reset();
		createdUsername = null;
		open = true;
		passwordForm.loadPolicy();
	}

	async function submit(event: SubmitEvent) {
		event.preventDefault();
		if (loading) return;
		// ユーザー名の欄にもエラーを出すため、先に validate() で送信済みにする。
		const passwordValid = passwordForm.validate();
		if (usernameError(username) !== undefined || !passwordValid) return;

		loading = true;
		const result = await createUser(username, passwordForm.password, asAdmin ? 'admin' : 'user');
		loading = false;
		if (!result.ok) {
			errorDialog?.show(m.admin_users_add_error_title(), result.message);
			return;
		}
		createdUsername = result.username;
		onadded();
	}
</script>

<Dialog.Root bind:open>
	<Dialog.Content bind:ref={content} {...closeGuard} onOpenAutoFocus={focusOnOpen}>
		{#if createdUsername !== null}
			<DialogResult
				title={m.admin_users_add_done_title()}
				description={m.admin_users_add_done_description({ username: createdUsername })}
			/>
		{:else}
			<DialogFormHeader
				title={m.admin_users_add_dialog_title()}
				description={m.admin_users_add_dialog_description()}
				closeDisabled={loading}
			/>
			{#if passwordForm.loadErrorMessage}
				<LoadError message={passwordForm.loadErrorMessage} onretry={passwordForm.loadPolicy} />
			{:else if passwordForm.policy === null}
				<LoadingIndicator />
			{:else}
				<form onsubmit={submit}>
					<Field.FieldGroup class="gap-3.5">
						<UsernameField
							idPrefix="add-user-username"
							autocomplete="off"
							bind:value={username}
							submitted={passwordForm.submitted}
						/>
						<NewPasswordFields
							policy={passwordForm.policy}
							passwordLabel={m.common_password_label()}
							confirmLabel={m.common_password_confirm_label()}
							idPrefix="add-user-password"
							bind:password={passwordForm.password}
							bind:confirmation={passwordForm.confirmation}
							submitted={passwordForm.submitted}
						/>
						<Field.Field orientation="horizontal" class="items-start gap-2.5">
							<Checkbox id="add-user-admin" bind:checked={asAdmin} />
							<div class="grid gap-1">
								<Field.FieldLabel for="add-user-admin" class="font-bold">
									{m.admin_users_add_role_label()}
								</Field.FieldLabel>
								<Field.FieldDescription>
									{m.admin_users_add_role_hint()}
								</Field.FieldDescription>
							</div>
						</Field.Field>
					</Field.FieldGroup>
					<DialogFormFooter {loading} oncancel={() => (open = false)}>
						<LoadingButton type="submit" {loading}>
							{m.admin_users_add_submit_button()}
						</LoadingButton>
					</DialogFormFooter>
				</form>
			{/if}
		{/if}
	</Dialog.Content>
</Dialog.Root>

<NoticeDialog bind:this={errorDialog} />
