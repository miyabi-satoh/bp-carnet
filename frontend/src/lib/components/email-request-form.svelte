<script lang="ts">
	import { tick, type Snippet } from 'svelte';
	import { halfWidthInput } from '$lib/half-width';
	import AuthCard from '$lib/components/auth-card.svelte';
	import * as Field from '$lib/components/ui/field';
	import { Input } from '$lib/components/ui/input';
	import LoadingButton from '$lib/components/loading-button.svelte';
	import EmailSentNotice from '$lib/components/email-sent-notice.svelte';
	import * as m from '$lib/paraglide/messages.js';
	import RequiredMark from '$lib/components/required-mark.svelte';

	type Props = {
		/** メール欄の下に出す補足。 */
		description?: string;
		submitLabel: string;
		/** 送信ボタンの直上に置くもの (サインアップの同意の文)。 */
		beforeSubmit?: Snippet;
		sentTitle: string;
		sentDescription: string;
		/** 送れたら `true` を返す。フォームを `sentTitle` の表示に差し替える。 */
		onsubmit: (email: string) => Promise<boolean>;
	};

	let { description, submitLabel, beforeSubmit, sentTitle, sentDescription, onsubmit }: Props =
		$props();

	let email = $state('');
	let loading = $state(false);
	let sent = $state(false);
	let emailInput = $state<HTMLInputElement | null>(null);

	/** 送った後の表示から入力に戻す。押したボタンが消えるので、メール欄に移る。 */
	async function editEmail() {
		sent = false;
		await tick();
		emailInput?.focus();
	}

	async function handleSubmit(event: SubmitEvent) {
		event.preventDefault();
		if (loading) return;
		loading = true;
		try {
			sent = await onsubmit(email);
		} finally {
			loading = false;
		}
	}
</script>

<!-- メールアドレスを送ってリンクを受け取る画面 (サインアップ・再設定の申し込み) のカード。
     送れたら同じ画面で「届いたメールを開く」ことを伝える。 -->
<AuthCard>
	{#if sent}
		<!-- 入れた値は残す。 -->
		<EmailSentNotice
			title={sentTitle}
			description={sentDescription}
			sentTo={email}
			appNote={m.email_request_sent_app_note()}
			onedit={editEmail}
		/>
	{:else}
		<form onsubmit={handleSubmit}>
			<Field.FieldGroup class="gap-5">
				<Field.Field>
					<Field.FieldLabel for="email">{m.common_email_label()}<RequiredMark /></Field.FieldLabel>
					<Input
						id="email"
						type="email"
						autocomplete="email"
						{@attach halfWidthInput}
						aria-describedby={description ? 'email-description' : undefined}
						bind:value={email}
						bind:ref={emailInput}
						required
					/>
					{#if description}
						<Field.FieldDescription id="email-description">{description}</Field.FieldDescription>
					{/if}
				</Field.Field>
				<Field.Field class="gap-2.5">
					{@render beforeSubmit?.()}
					<LoadingButton type="submit" class="w-full text-base" {loading}
						>{submitLabel}</LoadingButton
					>
				</Field.Field>
			</Field.FieldGroup>
		</form>
	{/if}
</AuthCard>
