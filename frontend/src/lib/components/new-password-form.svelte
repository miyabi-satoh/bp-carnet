<script lang="ts">
	import { onMount } from 'svelte';
	import * as Field from '$lib/components/ui/field';
	import AuthCard from '$lib/components/auth-card.svelte';
	import LoadError from '$lib/components/load-error.svelte';
	import LoadingButton from '$lib/components/loading-button.svelte';
	import LoadingIndicator from '$lib/components/loading-indicator.svelte';
	import NewPasswordFields from '$lib/components/new-password-fields.svelte';
	import { NewPasswordState } from '$lib/new-password.svelte';

	type Props = {
		passwordLabel: string;
		confirmLabel: string;
		submitLabel: string;
		/** 入力がポリシーを満たし、2欄が一致したときに呼ぶ。終わるまでボタンを送信中にする。 */
		onsubmit: (password: string) => Promise<void>;
	};

	let { passwordLabel, confirmLabel, submitLabel, onsubmit }: Props = $props();

	const passwordForm = new NewPasswordState();
	let loading = $state(false);

	onMount(() => {
		passwordForm.loadPolicy();
	});

	async function handleSubmit(event: SubmitEvent) {
		event.preventDefault();
		if (loading || !passwordForm.validate()) return;

		loading = true;
		try {
			await onsubmit(passwordForm.password);
		} finally {
			loading = false;
		}
	}
</script>

<!-- メールのリンクの先でパスワードを設定する画面 (サインアップの確認・再設定) のフォーム。
     デザインに合わせ、カード1枚に新しいパスワードと確認の2欄を置く。 -->
{#if passwordForm.loadErrorMessage}
	<LoadError message={passwordForm.loadErrorMessage} onretry={passwordForm.loadPolicy} />
{:else if passwordForm.policy === null}
	<LoadingIndicator />
{:else}
	<AuthCard>
		<form onsubmit={handleSubmit}>
			<Field.FieldGroup class="gap-3.5">
				<NewPasswordFields
					policy={passwordForm.policy}
					{passwordLabel}
					{confirmLabel}
					idPrefix="password"
					bind:password={passwordForm.password}
					bind:confirmation={passwordForm.confirmation}
					submitted={passwordForm.submitted}
				/>
				<Field.Field class="mt-1.5">
					<LoadingButton type="submit" class="w-full text-base" {loading}
						>{submitLabel}</LoadingButton
					>
				</Field.Field>
			</Field.FieldGroup>
		</form>
	</AuthCard>
{/if}
