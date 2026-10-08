<script lang="ts">
	import './layout.css';
	import favicon from '$lib/assets/favicon.svg';
	import { onMount } from 'svelte';
	import { page } from '$app/state';
	import { goto } from '$app/navigation';
	import { resolve } from '$app/paths';
	import { ModeWatcher, mode } from 'mode-watcher';
	import { fetchBetaNotice, isPublicRoute, userInitial } from '$lib/auth';
	import { isNativeApp, setStatusBarDark } from '$lib/native-app';
	import { startTopupSync } from '$lib/app-store-purchase';
	import * as Avatar from '$lib/components/ui/avatar';
	import * as DropdownMenu from '$lib/components/ui/dropdown-menu';
	import BetaBadge from '$lib/components/beta-badge.svelte';
	import ModeToggle from '$lib/components/mode-toggle.svelte';
	import LoadFailureScreen from '$lib/components/load-failure-screen.svelte';
	import AppUpdateScreen from '$lib/components/app-update-screen.svelte';
	import { appUpdate } from '$lib/app-update.svelte';
	import LogoutDialog from '$lib/components/logout-dialog.svelte';
	import NavigationProgress from '$lib/components/navigation-progress.svelte';
	import { pageTitle } from '$lib/page-title';
	import * as m from '$lib/paraglide/messages.js';

	let { children } = $props();

	/** 共通ヘッダーの代わりに、丸い戻るボタンと見出しを自前で置くページ
	 * (設定・ユーザー管理・写真で記録。利用者の閲覧ページはユーザー管理に揃える)。
	 * 印刷用レポートも、戻り方をほかの画面とそろえるためここに入れる (デザインから変えた)。
	 * 買い足しの購入結果は、見出しだけを置く (行き先はカードの中のボタン)。 */
	const OWN_HEADER_ROUTES: readonly string[] = [
		'/record/photo',
		'/report',
		'/settings',
		'/settings/import',
		'/settings/change-password',
		'/admin/users',
		'/admin/users/[id]',
		'/payments/ocr-topup/result'
	];

	let showHeader = $derived(
		!appUpdate.required &&
			!page.data.loadFailure &&
			!isPublicRoute(page.route.id) &&
			!OWN_HEADER_ROUTES.includes(page.route.id ?? '')
	);

	/** プロフィール画像が無い・読み込めないときにアバターに出す1文字。 */
	let avatarInitial = $derived(userInitial(page.data.user));

	/** 「オープンβテスト中」のバッジを出すか。 */
	let betaNotice = $state(false);
	let betaNoticeRequested = false;

	// ヘッダーを初めて出すときに1回だけ問い合わせる (`+layout.ts` の load は遷移のたびに走るため、そこには置かない)。
	// ログイン画面から入った場合もレイアウトは作り直されないので、ページを開いた1回で済む。
	$effect(() => {
		if (!showHeader || betaNoticeRequested) return;
		betaNoticeRequested = true;
		fetchBetaNotice().then((enabled) => (betaNotice = enabled));
	});

	// 画面のライト・ダークを端末と別に選べるので、ステータスバーの文字色も画面に合わせる。
	$effect(() => {
		if (mode.current === undefined) return;
		void setStatusBarDark(mode.current === 'dark').catch(() => {});
	});

	// アプリ内課金で、前の起動で送れなかった取引と起動中に届く取引をサーバーに送る (docs/payments.md
	// 「アプリ内課金」)。送るにはログインが要るので、ログインしてから始める。
	$effect(() => {
		if (isNativeApp && page.data.user) startTopupSync();
	});

	let logoutDialog = $state<ReturnType<typeof LogoutDialog> | null>(null);

	// docs/architecture.md: Service Worker はセキュアコンテキストでだけ登録する (URL を自前で判定しない)。
	// セキュアコンテキストでないときは登録処理ごと読み込まないよう、動的に import する。
	// アプリは画面を同梱しているので登録しない (docs/mobile-app.md)。
	onMount(() => {
		if (isNativeApp || !window.isSecureContext || !('serviceWorker' in navigator)) return;
		import('virtual:pwa-register').then(({ registerSW }) => registerSW({ immediate: true }));
	});
</script>

<svelte:head>
	<title>{pageTitle(page.route.id, page.params.slug)}</title>
	<link rel="icon" href={favicon} />
</svelte:head>

<ModeWatcher />
<NavigationProgress />

{#if showHeader}
	<header class="flex items-center justify-between p-5 pt-safe-5 pb-3 print:hidden">
		<div class="flex items-center gap-2">
			<a href={resolve('/')} class="text-xl font-bold text-foreground">{m.app_name()}</a>
			{#if betaNotice}
				<BetaBadge />
			{/if}
		</div>
		<div class="flex items-center gap-2">
			<ModeToggle />
			<!-- アバターのトリガーからログアウト・個人設定を開く。アカウント削除は
			     すぐ押せる場所に置かず、設定画面の中だけにする。 -->
			<DropdownMenu.Root>
				<DropdownMenu.Trigger aria-label={m.layout_user_menu_label()} class="rounded-full">
					<!-- デザインのアバターには枠線が無いため、Avatar 既定の枠 (after:border) を消す。 -->
					<Avatar.Root class="size-11 after:border-0">
						<!-- Google のプロフィール画像は、リファラ付きだと読み込みを拒まれることがあるため送らない。 -->
						<Avatar.Image
							src={page.data.user?.avatarUrl ?? undefined}
							alt=""
							referrerpolicy="no-referrer"
						/>
						<Avatar.Fallback class="bg-secondary text-base font-bold text-secondary-foreground">
							{avatarInitial}
						</Avatar.Fallback>
					</Avatar.Root>
				</DropdownMenu.Trigger>
				<!-- 既定の幅は開くボタン (アバター) に揃い、項目が折り返すため、中身の幅にする。 -->
				<DropdownMenu.Content align="end" class="w-auto">
					<DropdownMenu.Item onclick={() => goto(resolve('/settings'))}>
						{m.settings_title()}
					</DropdownMenu.Item>
					<DropdownMenu.Item onclick={() => goto(resolve('/help'))}>
						{m.help_title()}
					</DropdownMenu.Item>
					<!-- 管理画面の入口はここだけ (設定画面には置かない)。全画面から届き、管理者以外には
					     そもそも出さないため (docs/authentication.md)。 -->
					{#if page.data.user?.role === 'admin'}
						<DropdownMenu.Item onclick={() => goto(resolve('/admin/users'))}>
							{m.admin_users_menu_link()}
						</DropdownMenu.Item>
					{/if}
					<DropdownMenu.Separator />
					<DropdownMenu.Item onclick={() => logoutDialog?.show()}>
						{m.common_logout_button()}
					</DropdownMenu.Item>
				</DropdownMenu.Content>
			</DropdownMenu.Root>
		</div>
	</header>
{/if}
{#if appUpdate.required}
	<AppUpdateScreen />
{:else if page.data.loadFailure}
	<LoadFailureScreen failure={page.data.loadFailure} />
{:else}
	{@render children()}
{/if}

<LogoutDialog bind:this={logoutDialog} />
