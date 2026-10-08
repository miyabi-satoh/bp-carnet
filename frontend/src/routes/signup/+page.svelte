<script lang="ts">
	import { onMount } from 'svelte';
	import { fetchAuthProviders, type LoadFailure, requestSignup } from '$lib/auth';
	import AuthPageHeader from '$lib/components/auth-page-header.svelte';
	import BackToLoginLink from '$lib/components/back-to-login-link.svelte';
	import BetaNotice from '$lib/components/beta-notice.svelte';
	import CenteredScreen from '$lib/components/centered-screen.svelte';
	import EmailRequestForm from '$lib/components/email-request-form.svelte';
	import LoadingIndicator from '$lib/components/loading-indicator.svelte';
	import LoadFailureScreen from '$lib/components/load-failure-screen.svelte';
	import NoticeDialog from '$lib/components/notice-dialog.svelte';
	import TermsConsentNote from '$lib/components/terms-consent-note.svelte';
	import * as m from '$lib/paraglide/messages.js';
	import MailIcon from '@lucide/svelte/icons/mail';

	/** 使えるログイン方式を問い合わせ終えたか。 */
	let loaded = $state(false);
	/** 「オープンβテスト中」の一文を出す構成か。 */
	let betaNotice = $state(false);
	/** 使えるログイン方式を問い合わせられなかった理由。あれば画面の代わりに「もう一度試す」を出す。 */
	let loadFailure = $state<LoadFailure | null>(null);
	let errorDialog = $state<ReturnType<typeof NoticeDialog> | null>(null);

	async function loadProviders() {
		const result = await fetchAuthProviders();
		if (!result.ok) {
			loadFailure = result.failure;
			return;
		}
		loadFailure = null;
		betaNotice = result.data.betaNotice;
		loaded = true;
	}

	onMount(loadProviders);

	async function handleSubmit(email: string) {
		const result = await requestSignup(email);
		if (!result.ok) {
			errorDialog?.show(m.signup_error_title(), result.message);
		}
		return result.ok;
	}
</script>

{#if loadFailure}
	<LoadFailureScreen failure={loadFailure} onretry={loadProviders} />
{:else}
	<!-- デザインに無い画面のため、ResetPassword と同じ組み方 (見出し + カード1枚) に揃える。 -->
	<CenteredScreen>
		<AuthPageHeader icon={MailIcon} title={m.signup_title()} subtitle={m.signup_subtitle()} />

		{#if !loaded}
			<LoadingIndicator />
		{:else}
			{#if betaNotice}
				<BetaNotice />
			{/if}
			<EmailRequestForm
				submitLabel={m.signup_submit_button()}
				sentTitle={m.signup_sent_title()}
				sentDescription={m.signup_sent_description()}
				onsubmit={handleSubmit}
			>
				{#snippet beforeSubmit()}
					<!-- 確認メールは外国の事業者から送るので、パスワードの設定ではなくこの申し込みで同意をもらう
				     (docs/authentication.md)。 -->
					<TermsConsentNote message={m.terms_consent_signup} />
				{/snippet}
			</EmailRequestForm>
			<BackToLoginLink />
		{/if}
	</CenteredScreen>
{/if}

<!-- 失敗の画面に替わっても、出しているお知らせ (Google・LINE・Apple のログインの失敗など) を消さない。 -->
<NoticeDialog bind:this={errorDialog} />
