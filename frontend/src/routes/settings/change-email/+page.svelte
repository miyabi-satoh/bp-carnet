<script lang="ts">
	import { tick } from 'svelte';
	import { resolve } from '$app/paths';
	import { page } from '$app/state';
	import { requestEmailChange } from '$lib/auth';
	import { halfWidthInput } from '$lib/half-width';
	import * as Field from '$lib/components/ui/field';
	import { Input } from '$lib/components/ui/input';
	import LoadingButton from '$lib/components/loading-button.svelte';
	import NoticeDialog from '$lib/components/notice-dialog.svelte';
	import PageHeader from '$lib/components/page-header.svelte';
	import EmailSentNotice from '$lib/components/email-sent-notice.svelte';
	import PasswordInput from '$lib/components/password-input.svelte';
	import RequiredMark from '$lib/components/required-mark.svelte';
	import * as m from '$lib/paraglide/messages.js';

	/** パスワードを持つアカウントだけ、今のパスワードを求める (パスワードの変更と同じ理由。
	 * docs/authentication.md)。 */
	const passwordUsable = page.data.user?.passwordUsable ?? false;

	let newEmail = $state('');
	let currentPassword = $state('');
	let loading = $state(false);
	/** 送れたら、送った先を出して中身を差し替える。 */
	let sentTo = $state<string | null>(null);
	let errorDialog = $state<ReturnType<typeof NoticeDialog> | null>(null);
	let newEmailInput = $state<HTMLInputElement | null>(null);

	async function handleSubmit(event: SubmitEvent) {
		event.preventDefault();
		if (loading) return;

		loading = true;
		const email = newEmail.trim();
		const result = await requestEmailChange(email, passwordUsable ? currentPassword : null);
		loading = false;
		if (!result.ok) {
			errorDialog?.show(m.settings_change_email_error_title(), result.message);
			return;
		}
		sentTo = email;
	}

	/** 送り先を直す。入れた値は残し、パスワードは入れ直してもらう。押したボタンが消えるので、メール欄に移る。 */
	async function editEmail() {
		currentPassword = '';
		sentTo = null;
		await tick();
		newEmailInput?.focus();
	}
</script>

<!-- パスワードの変更 (settings/change-password) と同じく、カード1枚に欄を縦に並べる。 -->
<div class="mx-auto flex max-w-2xl flex-col px-5 pb-safe-5">
	<PageHeader backHref={resolve('/settings')} title={m.settings_change_email_button()} />

	<div class="rounded-lg raised bg-card p-6">
		{#if sentTo !== null}
			<EmailSentNotice
				title={m.settings_change_email_sent_title()}
				description={m.settings_change_email_sent_description()}
				{sentTo}
				appNote={m.settings_change_email_sent_app_note()}
				onedit={editEmail}
			/>
		{:else}
			<form onsubmit={handleSubmit}>
				<Field.FieldGroup class="gap-3.5">
					<Field.Field>
						<Field.FieldTitle>{m.settings_change_email_current_label()}</Field.FieldTitle>
						<p class="text-base break-all text-subtle-foreground">{page.data.user?.username}</p>
					</Field.Field>
					<Field.Field>
						<Field.FieldLabel for="new-email"
							>{m.settings_change_email_new_label()}<RequiredMark /></Field.FieldLabel
						>
						<Input
							id="new-email"
							type="email"
							autocomplete="email"
							{@attach halfWidthInput}
							bind:value={newEmail}
							bind:ref={newEmailInput}
							required
							aria-describedby="new-email-description"
						/>
						<Field.FieldDescription id="new-email-description"
							>{m.settings_change_email_new_description()}</Field.FieldDescription
						>
					</Field.Field>
					{#if passwordUsable}
						<Field.Field>
							<Field.FieldLabel for="current-password"
								>{m.settings_change_email_password_label()}<RequiredMark /></Field.FieldLabel
							>
							<PasswordInput
								id="current-password"
								autocomplete="current-password"
								bind:value={currentPassword}
								required
							/>
						</Field.Field>
					{/if}
					<Field.Field class="mt-1.5">
						<LoadingButton type="submit" class="w-full text-base" {loading}
							>{m.settings_change_email_submit_button()}</LoadingButton
						>
					</Field.Field>
				</Field.FieldGroup>
			</form>
		{/if}
	</div>
</div>

<NoticeDialog bind:this={errorDialog} />
