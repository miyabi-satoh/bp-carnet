<script lang="ts">
	import { onMount } from 'svelte';
	import { resolve } from '$app/paths';
	import { changePassword } from '$lib/auth';
	import * as Field from '$lib/components/ui/field';
	import LoadingButton from '$lib/components/loading-button.svelte';
	import LoadingIndicator from '$lib/components/loading-indicator.svelte';
	import LoadError from '$lib/components/load-error.svelte';
	import NewPasswordFields from '$lib/components/new-password-fields.svelte';
	import NoticeDialog from '$lib/components/notice-dialog.svelte';
	import PageHeader from '$lib/components/page-header.svelte';
	import PageResult from '$lib/components/page-result.svelte';
	import PasswordInput from '$lib/components/password-input.svelte';
	import { NewPasswordState } from '$lib/new-password.svelte';
	import * as m from '$lib/paraglide/messages.js';
	import RequiredMark from '$lib/components/required-mark.svelte';

	const passwordForm = new NewPasswordState();

	let currentPassword = $state('');
	let loading = $state(false);
	/** 変更できたら同じ画面の中身を結果に差し替える (設定画面へ戻すと、変わったことを
	 * 伝える場所が無くなる)。 */
	let changed = $state(false);
	let errorDialog = $state<ReturnType<typeof NoticeDialog> | null>(null);

	onMount(() => {
		passwordForm.loadPolicy();
	});

	async function handleSubmit(event: SubmitEvent) {
		event.preventDefault();
		if (loading || !passwordForm.validate()) return;

		loading = true;
		const result = await changePassword(currentPassword, passwordForm.password);
		loading = false;
		if (!result.ok) {
			errorDialog?.show(m.settings_change_password_error_title(), result.message);
			return;
		}
		changed = true;
	}
</script>

<!-- デザインに合わせ、カード1枚に欄を縦に並べる。現在のパスワードを
     求めるのは、セッションだけを奪った相手にパスワードを置き換えられないようにするため。 -->
<div class="mx-auto flex max-w-2xl flex-col px-5 pb-safe-5">
	<!-- 見出しは設定画面と同じ PageHeader (設定画面から続けて開くため)。 -->
	<PageHeader backHref={resolve('/settings')} title={m.settings_change_password_button()} />

	{#if changed}
		<!-- カードの枠はこの画面のフォームに合わせる。 -->
		<div class="rounded-lg raised bg-card p-6">
			<PageResult
				title={m.settings_change_password_done_title()}
				description={m.settings_change_password_done_description()}
			/>
		</div>
	{:else if passwordForm.loadErrorMessage}
		<LoadError message={passwordForm.loadErrorMessage} onretry={passwordForm.loadPolicy} />
	{:else if passwordForm.policy === null}
		<LoadingIndicator />
	{:else}
		<div class="rounded-lg raised bg-card p-6">
			<form onsubmit={handleSubmit}>
				<Field.FieldGroup class="gap-3.5">
					<Field.Field>
						<Field.FieldLabel for="current-password"
							>{m.settings_change_password_current_label()}<RequiredMark /></Field.FieldLabel
						>
						<PasswordInput
							id="current-password"
							autocomplete="current-password"
							bind:value={currentPassword}
							required
						/>
					</Field.Field>
					<NewPasswordFields
						policy={passwordForm.policy}
						passwordLabel={m.common_new_password_label()}
						confirmLabel={m.common_new_password_confirm_label()}
						idPrefix="password"
						bind:password={passwordForm.password}
						bind:confirmation={passwordForm.confirmation}
						submitted={passwordForm.submitted}
					/>
					<Field.Field class="mt-1.5">
						<LoadingButton type="submit" class="w-full text-base" {loading}
							>{m.settings_change_password_submit_button()}</LoadingButton
						>
					</Field.Field>
				</Field.FieldGroup>
			</form>
		</div>
	{/if}
</div>

<NoticeDialog bind:this={errorDialog} />
