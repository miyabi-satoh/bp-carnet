<script lang="ts">
	import { cn } from '#lib/utils.js';
	import { PAGE_CLASS } from '#lib/styles.js';
	import { onMount, type Snippet } from 'svelte';
	import { resolve } from '$app/paths';
	import { fetchAdminVerification, verifyAdminCode } from '#lib/admin.js';
	import { adminVerification } from '#lib/admin-verification.svelte.js';
	import { halfWidthInput } from '#lib/half-width.js';
	import FieldErrorList from '#lib/components/field-error-list.svelte';
	import * as Field from '#lib/components/ui/field/index.js';
	import { Input } from '#lib/components/ui/input/index.js';
	import LoadError from '#lib/components/load-error.svelte';
	import LoadingButton from '#lib/components/loading-button.svelte';
	import LoadingIndicator from '#lib/components/loading-indicator.svelte';
	import PageHeader from '#lib/components/page-header.svelte';
	import RequiredMark from '#lib/components/required-mark.svelte';
	import * as m from '#lib/paraglide/messages.js';

	/** 管理画面に入るときの確認コード (docs/authentication.md「管理者の確認コード」)。求める構成で、
	 * このログインでまだ通していなければ入力を出し、通したら中身 (管理画面) を出す。 */
	let { children }: { children: Snippet } = $props();

	type View = 'loading' | 'open' | 'code' | 'not-enrolled';
	let view = $state<View>('loading');
	// 開いている間に1時間が過ぎて管理 API に断られたら、入力に戻す。
	let shown = $derived<View>(adminVerification.expired ? 'code' : view);
	let loadErrorMessage = $state('');
	let code = $state('');
	let codeError = $state<string | undefined>();
	let submitting = $state(false);

	async function load() {
		loadErrorMessage = '';
		const result = await fetchAdminVerification();
		if (!result.ok) {
			loadErrorMessage = result.message;
			return;
		}
		// 開き直したときは、前に断られた印より今の状態を信じる。
		adminVerification.expired = false;
		const { required, enrolled, verified } = result.status;
		view = !required || verified ? 'open' : enrolled ? 'code' : 'not-enrolled';
	}

	async function handleSubmit(event: SubmitEvent) {
		event.preventDefault();
		if (submitting) return;
		submitting = true;
		codeError = undefined;
		try {
			const result = await verifyAdminCode(code);
			if (result.ok) {
				code = '';
				view = 'open';
				adminVerification.expired = false;
			} else if (result.code === 'admin_code_not_enrolled') {
				// 鍵を替えて登録が読めなくなったときも、ここに来る。
				view = 'not-enrolled';
				adminVerification.expired = false;
			} else {
				codeError = result.message;
			}
		} finally {
			submitting = false;
		}
	}

	onMount(load);
</script>

{#if shown === 'open'}
	{@render children()}
{:else}
	<div class={cn(PAGE_CLASS, 'pb-safe-5')}>
		<PageHeader backHref={resolve('/')} title={m.admin_code_title()} />
		{#if loadErrorMessage}
			<LoadError message={loadErrorMessage} onretry={load} />
		{:else if shown === 'loading'}
			<LoadingIndicator />
		{:else if shown === 'not-enrolled'}
			<p class="py-8 text-sm">{m.error_admin_code_not_enrolled()}</p>
		{:else}
			<form onsubmit={handleSubmit}>
				<Field.FieldGroup class="gap-5">
					<Field.Field>
						<Field.FieldLabel for="admin-code"
							>{m.admin_code_label()}<RequiredMark /></Field.FieldLabel
						>
						<!-- iPhone では、キーボードの上に「パスワード」アプリの確認コードが候補に出る (one-time-code)。 -->
						<Input
							id="admin-code"
							inputmode="numeric"
							autocomplete="one-time-code"
							{@attach halfWidthInput}
							aria-invalid={codeError ? true : undefined}
							aria-describedby={codeError ? 'admin-code-error' : undefined}
							bind:value={code}
							required
						/>
						<FieldErrorList id="admin-code-error" messages={[codeError]} />
					</Field.Field>
					<LoadingButton type="submit" class="w-full text-base" loading={submitting}
						>{m.admin_code_submit_button()}</LoadingButton
					>
				</Field.FieldGroup>
			</form>
		{/if}
	</div>
{/if}
