<script lang="ts">
	import { onMount } from 'svelte';
	import { goto } from '$app/navigation';
	import { resolve } from '$app/paths';
	import { page } from '$app/state';
	import { completeSignup, isEmailLinkUsable } from '$lib/auth';
	import AuthCard from '$lib/components/auth-card.svelte';
	import AuthPageHeader from '$lib/components/auth-page-header.svelte';
	import CenteredScreen from '$lib/components/centered-screen.svelte';
	import PageResult from '$lib/components/page-result.svelte';
	import BackToLoginLink from '$lib/components/back-to-login-link.svelte';
	import EmailLinkExpired from '$lib/components/email-link-expired.svelte';
	import LoadingIndicator from '$lib/components/loading-indicator.svelte';
	import NewPasswordForm from '$lib/components/new-password-form.svelte';
	import NoticeDialog from '$lib/components/notice-dialog.svelte';
	import * as m from '$lib/paraglide/messages.js';
	import LockIcon from '@lucide/svelte/icons/lock';

	/** 確認メールのリンクに載っているトークン。この画面の間は変わらない。 */
	const token = page.url.searchParams.get('token');
	/** アプリから申し込んだリンクか (`app=1`)。済んだらウェブのホーム画面へ進まず、アプリへ戻す。 */
	const fromApp = page.url.searchParams.get('app') === '1';

	/** アプリからの申し込みで設定できたら、フォームを結果に差し替える。 */
	let done = $state(false);

	/** リンクがまだ使えるか。`null` は問い合わせ中。 */
	let usable = $state<boolean | null>(null);

	let errorDialog = $state<ReturnType<typeof NoticeDialog> | null>(null);

	onMount(async () => {
		if (token !== null) usable = await isEmailLinkUsable('signup', token);
	});

	async function handleSubmit(password: string) {
		if (token === null) return;
		const result = await completeSignup(token, password);
		if (!result.ok) {
			// 入力している間に期限が切れたら、開いたときと同じく申し込み直す先へ導く。
			if (result.code === 'invalid_email_token') {
				usable = false;
				return;
			}
			errorDialog?.show(m.verify_email_error_title(), result.message);
			return;
		}
		if (fromApp) {
			done = true;
			return;
		}
		// 設定できた時点でログイン済みになっている。遷移が済むまでボタンは送信中のままにする。
		// この画面は履歴に残さない (戻る操作で使用済みのトークンのフォームに戻らないように)。
		await goto(resolve('/'), { replaceState: true });
	}
</script>

<CenteredScreen>
	<AuthPageHeader
		icon={LockIcon}
		title={m.verify_email_title()}
		subtitle={m.verify_email_subtitle()}
	/>

	{#if token === null}
		<AuthCard>
			<p class="text-sm">{m.verify_email_missing_token_description()}</p>
		</AuthCard>
	{:else if usable === null}
		<LoadingIndicator />
	{:else if !usable}
		<EmailLinkExpired
			description={m.verify_email_expired_description()}
			retryHref={resolve('/signup')}
			retryLabel={m.verify_email_expired_retry_button()}
		/>
	{:else if done}
		<AuthCard>
			<PageResult
				title={m.verify_email_done_title()}
				description={m.email_link_return_to_app_description()}
			/>
		</AuthCard>
	{:else}
		<NewPasswordForm
			passwordLabel={m.common_password_label()}
			confirmLabel={m.common_password_confirm_label()}
			submitLabel={m.verify_email_submit_button()}
			onsubmit={handleSubmit}
		/>
	{/if}
	<!-- アプリに戻る人に、ブラウザのログイン画面へのリンクは出さない。 -->
	{#if !done}
		<BackToLoginLink />
	{/if}

	<NoticeDialog bind:this={errorDialog} />
</CenteredScreen>
