<script lang="ts">
	import { halfWidthInput } from '$lib/half-width';
	import { onMount } from 'svelte';
	import { afterNavigate, goto, replaceState } from '$app/navigation';
	import { resolve } from '$app/paths';
	import { page } from '$app/state';
	import {
		contactlessMessage,
		GENERIC_ERROR_MESSAGE,
		needsContact,
		errorHelpSlug,
		oauthErrorMessage
	} from '$lib/api/errors';
	import {
		type AuthProviders,
		fetchAuthProviders,
		LINKED_PROVIDERS,
		type LoadFailure,
		type LinkedProvider,
		linkedProviderLabels,
		login,
		type LoginResult,
		loginWithApple,
		loginWithGoogle,
		loginWithLine
	} from '$lib/auth';
	import { isInAppBrowser } from '$lib/in-app-browser';
	import { isIosWebApp } from '$lib/ios-web-app';
	import { isNativeApp } from '$lib/native-app';
	import BetaBadge from '$lib/components/beta-badge.svelte';
	import AuthCard from '$lib/components/auth-card.svelte';
	import * as Field from '$lib/components/ui/field';
	import * as m from '$lib/paraglide/messages.js';
	import { Input } from '$lib/components/ui/input';
	import CenteredScreen from '$lib/components/centered-screen.svelte';
	import NoticeDialog from '$lib/components/notice-dialog.svelte';
	import PasswordInput from '$lib/components/password-input.svelte';
	import SignInButton from '$lib/components/sign-in-button.svelte';
	import { Button } from '$lib/components/ui/button';
	import LoadingButton from '$lib/components/loading-button.svelte';
	import LoadingIndicator from '$lib/components/loading-indicator.svelte';
	import LoadFailureScreen from '$lib/components/load-failure-screen.svelte';
	import TermsConsentNote from '$lib/components/terms-consent-note.svelte';
	import DropletIcon from '@lucide/svelte/icons/droplet';

	let username = $state('');
	let password = $state('');
	let loading = $state(false);
	/** 各社の SDK でログインしている間 (アプリ)。ID/パスワードのボタンにスピナーを出さず、押せなくするだけにする。 */
	let signingIn = $state(false);
	/** ID/パスワード・各社の SDK のどちらかでログインしている間。 */
	let signingInAny = $derived(loading || signingIn);
	/** 各社の SDK でのログインを始めた回数。最後に始めたものだけが `signingIn` を戻し、ログインを進める。 */
	let nativeSignInAttempt = 0;
	let errorDialog = $state<ReturnType<typeof NoticeDialog> | null>(null);
	/** 使えるログイン方式・サインアップの有無。`null` は問い合わせ中。 */
	let providers = $state<AuthProviders | null>(null);
	/** 使えるログイン方式を問い合わせられなかった理由。あれば画面の代わりに「もう一度試す」を出す。 */
	let loadFailure = $state<LoadFailure | null>(null);

	/** iOS のホーム画面から開いたアプリでは、LINE からの戻りがブラウザで開いて失敗するため、LINE のボタンを出さない (docs/authentication.md)。 */
	const lineBlocked = isIosWebApp();

	/** LINE・Google・Apple の戻り先やログインの画面自体が開けない内蔵ブラウザ (LINE のリンクからなど) では、
	 * ボタンの代わりにブラウザで開き直す案内を出す (docs/authentication.md)。 */
	const inAppBrowser = isInAppBrowser();
	/** アプリで SDK でログインできる提供元。ウェブのボタンはアプリの外の Safari を開いて戻れないので、
	 * ここに無いものは出さない (docs/mobile-app.md)。 */
	const NATIVE_SIGN_IN: Partial<Record<LinkedProvider, () => Promise<LoginResult | null>>> = {
		line: loginWithLine,
		google: loginWithGoogle,
		apple: loginWithApple
	};
	/** サーバーが受け付ける外部ログインか。アプリの Apple は、ウェブと要る設定が違うので別の値で見る。 */
	function providerEnabled(p: LinkedProvider, providers: AuthProviders): boolean {
		if (!isNativeApp) return providers[`${p}Enabled`];
		if (!NATIVE_SIGN_IN[p]) return false;
		return p === 'apple' ? providers.appleAppEnabled : providers[`${p}Enabled`];
	}
	/** 有効な外部ログイン (並びはボタンと同じ)。 */
	let enabledProviders = $derived(
		providers ? LINKED_PROVIDERS.filter((p) => providers && providerEnabled(p, providers)) : []
	);
	/** ボタンを出す外部ログイン。 */
	let shownProviders = $derived(
		inAppBrowser ? [] : enabledProviders.filter((p) => !(p === 'line' && lineBlocked))
	);

	const SIGN_IN_LABELS: Record<LinkedProvider, () => string> = {
		line: m.login_line_button,
		google: m.login_google_button,
		apple: m.login_apple_button
	};

	/** コピーの結果。押すまでは `null`。 */
	let copyResult = $state<'copied' | 'failed' | null>(null);

	async function copyLoginLink() {
		try {
			await navigator.clipboard.writeText(`${page.url.origin}${resolve('/login')}`);
			copyResult = 'copied';
		} catch {
			copyResult = 'failed';
		}
	}

	/** `helpSlug` を渡すと使い方の項目へのリンクを、`contact` を付けると問い合わせ先を出す。 */
	function showError(message: string, options: { helpSlug?: string; contact?: boolean } = {}) {
		errorDialog?.show(m.login_error_title(), message, options);
	}

	async function loadScreen() {
		const result = await fetchAuthProviders();
		if (!result.ok) {
			loadFailure = result.failure;
			return;
		}
		loadFailure = null;
		providers = result.data;
	}

	onMount(loadScreen);

	// Google・LINE・Apple のコールバックからの失敗リダイレクト (`/login?oauthError=...`) を検出して表示する。
	// リロード時に再表示されないよう、表示後は URL からクエリを取り除く。
	// `onMount` ではなく `afterNavigate` なのは、初回表示の `onMount` 時点ではまだ router が
	// 初期化されておらず、`replaceState` が "before router is initialized" で落ちるため。
	afterNavigate(() => {
		const code = page.url.searchParams.get('oauthError');
		const message = oauthErrorMessage(code);
		if (message === undefined) return;
		showError(message, { helpSlug: errorHelpSlug(code), contact: needsContact(code) });
		replaceState(page.url.pathname, {});
	});

	async function handleSubmit(e: SubmitEvent) {
		e.preventDefault();
		if (loading || signingIn) return;
		loading = true;

		try {
			await finishLogin(await login(username, password));
		} finally {
			loading = false;
		}
	}

	/** アプリで各社の SDK でログインする。画面を閉じられたら何もしない。 */
	async function handleNativeSignIn(
		provider: LinkedProvider,
		signIn: () => Promise<LoginResult | null>
	) {
		if (loading || signingIn) return;
		signingIn = true;
		const attempt = ++nativeSignInAttempt;
		// Google の SDK は、端末のロックなどで認証の画面が閉じられると結果を返さない (GoogleSignIn-iOS #578)。
		// ボタンが押せないまま残らないよう、アプリに戻ったら押せるように戻す。押し直したら、前の結果は後から届いても使わない。
		// LINE は戻さない。前のログインが終わる前に次を始めると、SDK はそれを無視して結果を返さないため。
		const release = () => {
			if (document.visibilityState === 'visible' && attempt === nativeSignInAttempt)
				signingIn = false;
		};
		if (provider === 'google') document.addEventListener('visibilitychange', release);
		try {
			const result = await signIn();
			if (result !== null && attempt === nativeSignInAttempt) await finishLogin(result);
		} finally {
			if (attempt === nativeSignInAttempt) signingIn = false;
			document.removeEventListener('visibilitychange', release);
		}
	}

	async function finishLogin(result: LoginResult) {
		if (!result.ok) {
			// 凍結・衝突は使い方では直せないのでリンクせず、問い合わせ先を出す (docs/authentication.md)。
			if (result.code && needsContact(result.code))
				showError(contactlessMessage(result.code), { contact: true });
			else showError(result.message, { helpSlug: errorHelpSlug(result.code) ?? 'trouble-login' });
			return;
		}
		// 削除予約の取り消しはホーム画面で知らせる。Google コールバックが返す URL
		// (`/?deletionCancelled=1`) に合わせ、ID/PW ログインでも同じクエリで渡す。
		// TODO: ログイン後の遷移先が決まったら差し替える
		const home = resolve('/');
		const target = result.deletionCancelled ? `${home}?deletionCancelled=1` : home;
		try {
			// クエリ付きのURLは resolve() の戻り値の型にならないため、この規則では追えない。
			// ログイン画面は履歴に残さない (戻る操作でログイン画面に戻らないように)。
			// eslint-disable-next-line svelte/no-navigation-without-resolve
			await goto(target, { replaceState: true });
		} catch {
			showError(GENERIC_ERROR_MESSAGE());
		}
	}

	/** フォームの下に並ぶリンク (再設定・作成)。見た目を揃えるため1か所で持つ。 */
	const FORM_LINK_CLASS =
		'inline-flex min-h-6 items-center text-accent-foreground hover:text-primary';
</script>

{#if loadFailure}
	<LoadFailureScreen failure={loadFailure} onretry={loadScreen} />
{:else}
	<CenteredScreen inert={signingInAny}>
		<!-- アイコンとアプリ名・サブタイトルは横に並べる (デザインは縦積み。
	     縦を詰めるために変えた。docs/authentication.md)。 -->
		<div class="mb-4 flex w-full max-w-80 items-center justify-center gap-4">
			<div
				class="flex size-14 shrink-0 items-center justify-center rounded-full bg-secondary text-secondary-foreground"
			>
				<DropletIcon class="size-7" aria-hidden="true" />
			</div>
			<div>
				<div class="flex items-center gap-2">
					<h1 class="text-2xl font-bold">{m.app_name()}</h1>
					{#if providers?.betaNotice}
						<BetaBadge />
					{/if}
				</div>
				<p class="text-sm text-muted-foreground">{m.login_subtitle()}</p>
			</div>
		</div>

		<!-- LINE・Google・Apple のボタンはフォームの上にあるため、使えるかどうかが分かるまでカードを出さない
	     (後から差し込むと、入力中のフォームが下にずれる)。その間はスピナーを出す。 -->
		{#if providers === null}
			<LoadingIndicator />
		{:else}
			<AuthCard>
				{@const anyEnabled = enabledProviders.length > 0}
				{@const anyShown = shownProviders.length > 0}
				{#if inAppBrowser && anyEnabled}
					<div class="mb-5 flex flex-col gap-3 text-sm text-muted-foreground">
						<p>
							{m.login_in_app_browser_notice({
								services: linkedProviderLabels(enabledProviders)
							})}
						</p>
						<Button type="button" variant="outline" class="w-full" onclick={copyLoginLink}
							>{m.login_in_app_browser_copy_button()}</Button
						>
						<!-- 読み上げに乗せるため、通知の要素は最初から置き、中身だけを差し替える。 -->
						<p role="status">
							{#if copyResult !== null}
								{copyResult === 'copied'
									? m.login_in_app_browser_copied()
									: m.login_in_app_browser_copy_failed()}
							{/if}
						</p>
					</div>
				{/if}
				{#if anyEnabled}
					{#if providers.lineEnabled && lineBlocked && !inAppBrowser}
						<!-- 代わりに勧めるのは、出ているボタン (Google・Apple) だけ。 -->
						<p class="mb-5 text-sm text-muted-foreground">
							{anyShown
								? m.login_line_ios_web_app_notice({
										services: linkedProviderLabels(shownProviders)
									})
								: m.login_line_ios_web_app_notice_browser_only()}
						</p>
					{/if}
					<!-- 初めての人には LINE・Google・Apple のボタンが登録になるので、直上に同意の文を1つ置く。
				     出ているボタンが無ければ (LINE を案内に置き換えて他も無い) 同意の文も区切りも出さない。 -->
					{#if anyShown}
						<div class="mb-2.5">
							<TermsConsentNote message={m.terms_consent_providers} />
						</div>
					{/if}
					{#if anyShown}
						<div class="flex flex-col gap-2.5">
							{#each shownProviders as provider (provider)}
								{@const nativeSignIn = isNativeApp ? NATIVE_SIGN_IN[provider] : undefined}
								<SignInButton
									{provider}
									label={SIGN_IN_LABELS[provider]()}
									onclick={nativeSignIn && (() => handleNativeSignIn(provider, nativeSignIn))}
									disabled={loading || signingIn}
								/>
							{/each}
						</div>
					{/if}
					{#if anyShown}
						<!-- Field.FieldSeparator は文言の地が bg-background 固定で、カードの上では地が浮くため自前で組む。 -->
						<div class="mt-5 mb-3 flex items-center gap-3 text-xs text-subtle-foreground">
							<div class="h-px flex-1 bg-border" aria-hidden="true"></div>
							{m.login_separator()}
							<div class="h-px flex-1 bg-border" aria-hidden="true"></div>
						</div>
					{/if}
				{/if}
				<form onsubmit={handleSubmit}>
					<!-- 間隔はデザインに合わせる (欄の間 14px、パスワード欄とボタンの間 20px、
				     ボタンとリンクの間 16px)。 -->
					<Field.FieldGroup class="gap-3.5">
						<Field.Field>
							<!-- サインアップしたユーザーのIDはメールアドレスそのものなので、そう書く (docs/authentication.md)。 -->
							<Field.FieldLabel for="username">{m.common_email_label()}</Field.FieldLabel>
							<Input
								id="username"
								autocomplete="username"
								inputmode="email"
								autocapitalize="off"
								bind:value={username}
								required
								{@attach halfWidthInput}
							/>
						</Field.Field>
						<Field.Field>
							<Field.FieldLabel for="password">{m.common_password_label()}</Field.FieldLabel>
							<PasswordInput
								id="password"
								autocomplete="current-password"
								bind:value={password}
								required
							/>
						</Field.Field>
						<Field.Field class="mt-1.5 gap-4">
							<LoadingButton type="submit" class="w-full text-base" {loading} disabled={signingIn}
								>{m.login_submit_button()}</LoadingButton
							>
							<div class="flex flex-col items-center gap-2 text-sm">
								<a href={resolve('/reset-password')} class={FORM_LINK_CLASS}>
									{m.login_forgot_password_link()}
								</a>
								<!-- デザインに無いリンク。 -->
								<a href={resolve('/signup')} class={FORM_LINK_CLASS}>
									{m.login_signup_link()}
								</a>
							</div>
						</Field.Field>
					</Field.FieldGroup>
				</form>
			</AuthCard>
			<!-- 読むもの (紹介・使い方・規約) はカードの下にまとめる。規約は登録する前にも読めるようにするため。使い方はデザインではカードの中だが、アカウントの操作
		     (再設定・作成) と分けるためここへ移した (docs/authentication.md)。 -->
			<nav
				class="mt-3 flex flex-wrap items-center justify-center gap-x-1.5 text-xs text-muted-foreground"
				aria-label={m.login_footer_links_label()}
			>
				<!-- アプリには出さない (docs/mobile-app.md)。紹介ページからこの画面へ戻れるので、同じタブで開く。 -->
				{#if providers.introUrl && !isNativeApp}
					<!-- eslint-disable svelte/no-navigation-without-resolve -->
					<a
						href={providers.introUrl}
						class="inline-flex min-h-6 items-center whitespace-nowrap hover:text-primary"
						>{m.login_intro_link()}</a
					>
					<!-- eslint-enable svelte/no-navigation-without-resolve -->
					<span aria-hidden="true">·</span>
				{/if}
				<a
					href={resolve('/help')}
					class="inline-flex min-h-6 items-center whitespace-nowrap hover:text-primary"
					>{m.login_help_link()}</a
				>
				<span aria-hidden="true">·</span>
				<a
					href={resolve('/terms')}
					class="inline-flex min-h-6 items-center whitespace-nowrap hover:text-primary"
					>{m.terms_title()}</a
				>
				<span aria-hidden="true">·</span>
				<a
					href={resolve('/privacy')}
					class="inline-flex min-h-6 items-center whitespace-nowrap hover:text-primary"
					>{m.privacy_title()}</a
				>
			</nav>
		{/if}
	</CenteredScreen>
{/if}

<!-- ログインの結果を待つ間は画面全体を覆う。ボタンが押せなくなるだけだと、止まって見えるため。
     送信は通信の共通の期限 (`REQUEST_TIMEOUT_MS`) で打ち切られ、失敗として閉じる。 -->
{#if signingInAny}
	<div
		class="fixed inset-0 z-50 flex items-center justify-center bg-background/80 backdrop-blur-xs"
	>
		<LoadingIndicator label={m.login_signing_in()} class="text-base text-foreground" />
	</div>
{/if}

<!-- 失敗の画面に替わっても、出しているお知らせ (Google・LINE・Apple のログインの失敗など) を消さない。 -->
<NoticeDialog bind:this={errorDialog} contactUrl={providers?.contactUrl} />
