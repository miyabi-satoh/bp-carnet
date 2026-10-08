<script lang="ts">
	import { onMount } from 'svelte';
	import { resolve } from '$app/paths';
	import { page } from '$app/state';
	import { completeEmailChange, emailChangeTarget } from '$lib/auth';
	import AuthCard from '$lib/components/auth-card.svelte';
	import AuthPageHeader from '$lib/components/auth-page-header.svelte';
	import CenteredScreen from '$lib/components/centered-screen.svelte';
	import EmailLinkExpired from '$lib/components/email-link-expired.svelte';
	import LoadError from '$lib/components/load-error.svelte';
	import LoadingButton from '$lib/components/loading-button.svelte';
	import LoadingIndicator from '$lib/components/loading-indicator.svelte';
	import NoticeDialog from '$lib/components/notice-dialog.svelte';
	import PageResult from '$lib/components/page-result.svelte';
	import { Button } from '$lib/components/ui/button';
	import * as m from '$lib/paraglide/messages.js';
	import MailIcon from '@lucide/svelte/icons/mail';

	/** メールアドレスの変更のリンクに載っているトークン。この画面の間は変わらない。 */
	const token = page.url.searchParams.get('token');
	/** アプリから申し込んだリンクか (`app=1`)。済んだらホーム画面ではなく、アプリへ戻す。 */
	const fromApp = page.url.searchParams.get('app') === '1';

	/** 変更先。`undefined` は問い合わせ中、`null` はリンクが使えない。 */
	let newEmail = $state<string | null | undefined>(undefined);
	/** 問い合わせに失敗したときの文言。 */
	let loadError = $state<string | null>(null);
	let loading = $state(false);
	let done = $state(false);
	let errorDialog = $state<ReturnType<typeof NoticeDialog> | null>(null);

	async function load() {
		if (token === null) return;
		loadError = null;
		newEmail = undefined;
		const result = await emailChangeTarget(token);
		if (result.ok) {
			newEmail = result.data.newEmail;
		} else if (result.code === 'invalid_email_token') {
			newEmail = null;
		} else {
			loadError = result.message;
		}
	}

	onMount(load);

	/** メールのリンクを開いただけでは変えず、ボタンで確かめてから変える (メールの安全確認の
	 * ためにリンクを先に開くサービスがあり、開いただけで確定すると本人の確認にならないため)。 */
	async function handleSubmit(event: SubmitEvent) {
		event.preventDefault();
		if (token === null || loading) return;
		loading = true;
		const result = await completeEmailChange(token);
		loading = false;
		if (result.ok) {
			done = true;
			return;
		}
		// 確かめている間に期限が切れたら、開いたときと同じく申し込み直す先へ導く。
		if (result.code === 'invalid_email_token') {
			newEmail = null;
			return;
		}
		errorDialog?.show(
			m.change_email_error_title(),
			result.code === 'username_taken' ? m.change_email_taken_description() : result.message
		);
	}
</script>

<CenteredScreen>
	<AuthPageHeader
		icon={MailIcon}
		title={m.change_email_title()}
		subtitle={m.change_email_header_subtitle()}
	/>

	{#if token === null}
		<AuthCard>
			<p class="text-sm">{m.change_email_missing_token_description()}</p>
		</AuthCard>
	{:else if loadError !== null}
		<LoadError message={loadError} onretry={load} />
	{:else if newEmail === undefined}
		<LoadingIndicator />
	{:else if newEmail === null}
		<EmailLinkExpired
			description={m.change_email_expired_description()}
			retryHref={resolve('/settings/change-email')}
			retryLabel={m.change_email_expired_retry_button()}
		/>
	{:else if done}
		<AuthCard>
			<PageResult
				title={m.change_email_done_title()}
				description={fromApp
					? m.change_email_done_app_description()
					: m.change_email_done_description()}
			/>
			{#if !fromApp}
				<Button href={resolve('/')} class="mt-5 w-full text-base"
					>{m.change_email_home_button()}</Button
				>
			{/if}
		</AuthCard>
	{:else}
		<AuthCard>
			<form onsubmit={handleSubmit}>
				<p class="text-sm text-muted-foreground">{m.change_email_subtitle()}</p>
				<p class="mt-2 text-lg font-bold break-all">{newEmail}</p>
				<LoadingButton type="submit" class="mt-5 w-full text-base" {loading}
					>{m.change_email_submit_button()}</LoadingButton
				>
			</form>
		</AuthCard>
	{/if}

	<NoticeDialog bind:this={errorDialog} />
</CenteredScreen>
