<script lang="ts">
	import { onMount } from 'svelte';
	import { resolve } from '$app/paths';
	import { page } from '$app/state';
	import { completePasswordReset, isEmailLinkUsable, requestPasswordReset } from '$lib/auth';
	import AuthCard from '$lib/components/auth-card.svelte';
	import AuthPageHeader from '$lib/components/auth-page-header.svelte';
	import BackToLoginLink from '$lib/components/back-to-login-link.svelte';
	import CenteredScreen from '$lib/components/centered-screen.svelte';
	import EmailLinkExpired from '$lib/components/email-link-expired.svelte';
	import EmailRequestForm from '$lib/components/email-request-form.svelte';
	import LoadingIndicator from '$lib/components/loading-indicator.svelte';
	import NewPasswordForm from '$lib/components/new-password-form.svelte';
	import NoticeDialog from '$lib/components/notice-dialog.svelte';
	import PageResult from '$lib/components/page-result.svelte';
	import * as m from '$lib/paraglide/messages.js';
	import LockIcon from '@lucide/svelte/icons/lock';

	/** 再設定のメールのリンクに載っているトークン。無ければ再設定の申し込みを受け付ける。 */
	const token = page.url.searchParams.get('token');
	/** アプリから申し込んだリンクか (`app=1`)。済んだらアプリに戻ってログインするよう案内する。 */
	const fromApp = page.url.searchParams.get('app') === '1';

	/** リンクがまだ使えるか。`null` は問い合わせ中 (リンクから開いたときだけ使う)。 */
	let usable = $state<boolean | null>(null);
	/** 再設定できたらフォームを結果に差し替える。 */
	let done = $state(false);
	let errorDialog = $state<ReturnType<typeof NoticeDialog> | null>(null);

	onMount(async () => {
		if (token !== null) usable = await isEmailLinkUsable('password-reset', token);
	});

	async function handleRequest(email: string) {
		const result = await requestPasswordReset(email);
		if (!result.ok) {
			errorDialog?.show(m.reset_password_request_error_title(), result.message);
		}
		return result.ok;
	}

	async function handleComplete(password: string) {
		if (token === null) return;
		const result = await completePasswordReset(token, password);
		if (!result.ok) {
			// 入力している間に期限が切れたら、開いたときと同じく申し込み直す先へ導く。
			if (result.code === 'invalid_email_token') {
				usable = false;
				return;
			}
			errorDialog?.show(m.reset_password_error_title(), result.message);
			return;
		}
		done = true;
	}
</script>

<CenteredScreen>
	{#if token === null}
		<AuthPageHeader
			icon={LockIcon}
			title={m.reset_password_title()}
			subtitle={m.reset_password_request_subtitle()}
		/>
		<EmailRequestForm
			description={m.reset_password_request_description()}
			submitLabel={m.reset_password_request_submit_button()}
			sentTitle={m.reset_password_sent_title()}
			sentDescription={m.reset_password_sent_description()}
			onsubmit={handleRequest}
		/>
	{:else}
		<AuthPageHeader
			icon={LockIcon}
			title={m.reset_password_title()}
			subtitle={m.reset_password_subtitle()}
		/>
		{#if usable === null}
			<LoadingIndicator />
		{:else if !usable}
			<EmailLinkExpired
				description={m.reset_password_expired_description()}
				retryHref={resolve('/reset-password')}
				retryLabel={m.reset_password_expired_retry_button()}
			/>
		{:else if done}
			<AuthCard>
				<PageResult
					title={m.reset_password_done_title()}
					description={fromApp
						? m.email_link_return_to_app_description()
						: m.reset_password_done_description()}
				/>
			</AuthCard>
		{:else}
			<NewPasswordForm
				passwordLabel={m.common_new_password_label()}
				confirmLabel={m.common_new_password_confirm_label()}
				submitLabel={m.reset_password_submit_button()}
				onsubmit={handleComplete}
			/>
		{/if}
	{/if}
	<!-- アプリに戻る人に、ブラウザのログイン画面へのリンクは出さない。 -->
	{#if !(done && fromApp)}
		<BackToLoginLink />
	{/if}
</CenteredScreen>

<NoticeDialog bind:this={errorDialog} />
