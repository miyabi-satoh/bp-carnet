<script lang="ts">
	import { onMount } from 'svelte';
	import { page } from '$app/state';
	import { resolve } from '$app/paths';
	import {
		adminUserName,
		adminUserStatusLabel,
		cancelUserDeletion,
		fetchAdminUsers,
		setUserFrozen,
		type AdminUser
	} from '$lib/admin';
	import type { ApiResult } from '$lib/api/errors';
	import { userTimeZone } from '$lib/auth';
	import { filterAdminUsers } from '$lib/admin-search';
	import { Button } from '$lib/components/ui/button';
	import { Input } from '$lib/components/ui/input';
	import AdminAddUserDialog from '$lib/components/admin-add-user-dialog.svelte';
	import AdminDeleteUserDialog from '$lib/components/admin-delete-user-dialog.svelte';
	import AdminOcrLimitDialog from '$lib/components/admin-ocr-limit-dialog.svelte';
	import AdminResetPasswordDialog from '$lib/components/admin-reset-password-dialog.svelte';
	import LoadingButton from '$lib/components/loading-button.svelte';
	import LoadError from '$lib/components/load-error.svelte';
	import LoadingIndicator from '$lib/components/loading-indicator.svelte';
	import NoticeDialog from '$lib/components/notice-dialog.svelte';
	import PageHeader from '$lib/components/page-header.svelte';
	import * as m from '$lib/paraglide/messages.js';
	import { formatDateLabel, formatFutureDateLabel } from '$lib/period';
	import ChevronRightIcon from '@lucide/svelte/icons/chevron-right';
	import PlusIcon from '@lucide/svelte/icons/plus';
	import SearchIcon from '@lucide/svelte/icons/search';
	import Trash2Icon from '@lucide/svelte/icons/trash-2';

	let users = $state<AdminUser[] | null>(null);
	let ocrDefaultBudgetYen = $state(0);
	let loadErrorMessage = $state('');

	/** 一覧の絞り込み (ユーザー ID・名前)。一覧は全員を一度に取るので、画面の中で絞る。 */
	let query = $state('');
	let shownUsers = $derived(users === null ? [] : filterAdminUsers(users, query));

	let errorDialog = $state<ReturnType<typeof NoticeDialog> | null>(null);
	let ocrLimitDialog = $state<ReturnType<typeof AdminOcrLimitDialog> | null>(null);
	let resetPasswordDialog = $state<ReturnType<typeof AdminResetPasswordDialog> | null>(null);
	let deleteDialog = $state<ReturnType<typeof AdminDeleteUserDialog> | null>(null);
	let addDialog = $state<ReturnType<typeof AdminAddUserDialog> | null>(null);

	/** カードのボタン (凍結・解除、削除の取り消し) で操作中のユーザーの id。同時に1件しか操作しない。 */
	let pendingUserId = $state<number | null>(null);

	function isSelf(user: AdminUser): boolean {
		return user.id === page.data.user?.id;
	}

	/** 自分自身は凍結できない (凍結した時点で管理画面から締め出される)。サーバーも拒むが、
	 * 押してから失敗を知るより、押せない理由が見えている方が分かりやすい。 */
	function selfFreezeBlocked(user: AdminUser): boolean {
		return !user.frozen && isSelf(user);
	}

	/** カードのボタンの操作を1件だけ走らせ、一覧を取り直す。1行だけ差し替えないのは、凍結の可否が
	 * 他の行の状態にも依存するため。取り直しが終わるまで押せないままにする (古い表示のまま
	 * 二度押しさせないため)。 */
	async function runCardAction(
		user: AdminUser,
		action: () => Promise<ApiResult>,
		{ errorTitle, reloadOnError }: { errorTitle: string; reloadOnError: boolean }
	) {
		if (pendingUserId !== null) return;
		pendingUserId = user.id;
		const result = await action();
		if (result.ok || reloadOnError) await load();
		pendingUserId = null;
		if (!result.ok) errorDialog?.show(errorTitle, result.message);
	}

	function toggleFrozen(user: AdminUser) {
		return runCardAction(user, () => setUserFrozen(user.id, !user.frozen), {
			errorTitle: m.admin_users_freeze_error_title(),
			reloadOnError: false
		});
	}

	/** 失敗しても取り直す: 拒まれるのは表示が古く、予約の起点がサーバーとずれているとき。 */
	function cancelDeletion(user: AdminUser) {
		return runCardAction(user, () => cancelUserDeletion(user.id), {
			errorTitle: m.admin_users_cancel_deletion_error_title(),
			reloadOnError: true
		});
	}

	/** 個別指定が無ければ設定の既定値を使う。負値は無制限。 */
	function ocrLimitLabel(user: AdminUser): string {
		const budget = user.ocrBudgetYen ?? ocrDefaultBudgetYen;
		return budget < 0
			? m.admin_users_ocr_limit_summary_unlimited({ spent: user.ocrSpentYen })
			: m.admin_users_ocr_limit_summary({ budget, spent: user.ocrSpentYen });
	}

	async function load() {
		loadErrorMessage = '';
		const result = await fetchAdminUsers();
		if (!result.ok) {
			loadErrorMessage = result.message;
			return;
		}
		users = result.users;
		ocrDefaultBudgetYen = result.ocrDefaultBudgetYen;
	}

	onMount(load);
</script>

<!-- デザインどおり、共通ヘッダーの代わりに、丸い戻るボタンと見出し、右に「追加...」を置く。 -->
<div class="mx-auto flex max-w-2xl flex-col px-5 pb-safe-5">
	<PageHeader backHref={resolve('/')} title={m.admin_users_title()}>
		{#snippet actions()}
			<Button
				class="h-11 gap-1.5 rounded-full px-4 text-sm font-bold"
				onclick={() => addDialog?.show()}
			>
				<PlusIcon size={15} strokeWidth={2.5} aria-hidden="true" />
				{m.admin_users_add_button()}
			</Button>
		{/snippet}
	</PageHeader>

	{#if loadErrorMessage}
		<LoadError message={loadErrorMessage} onretry={load} />
	{:else if users === null}
		<LoadingIndicator />
	{:else if users.length === 0}
		<p class="py-8 text-center text-sm text-muted-foreground">{m.admin_users_empty()}</p>
	{:else}
		<!-- デザインには無い検索の欄。利用者が増えても探せるように (docs/authentication.md)。 -->
		<div class="mb-4 flex flex-col gap-2">
			<div class="relative">
				<SearchIcon
					size={18}
					strokeWidth={2.2}
					class="pointer-events-none absolute top-1/2 left-3.5 -translate-y-1/2 text-subtle-foreground"
					aria-hidden="true"
				/>
				<Input
					type="search"
					class="h-12 pl-10 text-base"
					aria-label={m.admin_users_search_label()}
					aria-describedby="admin-users-count"
					autocomplete="off"
					bind:value={query}
				/>
			</div>
			<p id="admin-users-count" class="text-sm text-muted-foreground" aria-live="polite">
				{query.trim() === ''
					? m.admin_users_count({ total: users.length })
					: m.admin_users_filtered_count({ total: users.length, shown: shownUsers.length })}
			</p>
		</div>
		{#if shownUsers.length === 0}
			<p class="py-8 text-center text-sm text-muted-foreground">{m.admin_users_no_match()}</p>
		{/if}
		<ul class="flex flex-col gap-2.5">
			{#each shownUsers as user (user.id)}
				{@const name = adminUserName(user)}
				{@const blocked = selfFreezeBlocked(user)}
				<li class="rounded-lg raised bg-card px-4 py-3.5">
					<!-- 見出しの行を、アカウント情報と記録の閲覧ページへの入口にする (OCR上限の行と同じく、
					     カードに行を増やさないため)。 -->
					<a
						href={resolve('/admin/users/[id]', { id: String(user.id) })}
						class="-mx-2 mb-2 flex min-h-11 items-center justify-between gap-2 rounded-md px-2 transition-colors outline-none hover:bg-muted focus-visible:ring-3 focus-visible:ring-ring/50"
					>
						<span class="text-base font-bold">{name}</span>
						<span class="flex shrink-0 items-center gap-1">
							<span
								class={[
									'rounded-full px-2.5 py-0.5 text-xs font-bold',
									user.deletionScheduledAt
										? 'bg-destructive/10 text-destructive'
										: user.frozen
											? 'bg-background text-muted-foreground'
											: 'bg-secondary text-secondary-foreground'
								]}
							>
								{adminUserStatusLabel(user)}
							</span>
							<ChevronRightIcon
								size={16}
								strokeWidth={2.2}
								class="text-subtle-foreground"
								aria-hidden="true"
							/>
						</span>
					</a>
					<!-- デザインのOCR上限の行を、そのまま変更モーダルの入口にする
					     (行が増えるとカードの情報量が増えるため)。 -->
					<Button
						variant="ghost"
						class="-mx-2 mb-2 flex h-11 w-full items-center justify-between px-2 text-xs font-normal text-muted-foreground"
						aria-label={m.admin_users_ocr_limit_edit_label({ name })}
						onclick={() => ocrLimitDialog?.show(user, ocrDefaultBudgetYen)}
					>
						<span>{ocrLimitLabel(user)}</span>
						<span class="flex items-center gap-0.5 font-bold text-foreground">
							{m.admin_users_ocr_limit_edit_button()}
							<ChevronRightIcon size={14} strokeWidth={2.2} aria-hidden="true" />
						</span>
					</Button>
					<!-- 買い足した枠は、残っているときだけ出す。 -->
					{#if user.ocrPaidRemainingYen > 0}
						<p class="mb-2 text-xs text-muted-foreground">
							{m.admin_users_ocr_paid_quota({ remaining: user.ocrPaidRemainingYen })}
						</p>
					{/if}
					<p class="mb-2 text-xs text-muted-foreground">
						{m.admin_users_last_seen({
							date: formatDateLabel(new Date(user.lastSeenAt), true, userTimeZone())
						})}
					</p>
					<div class="flex gap-2">
						{#if user.frozen}
							<LoadingButton
								variant="outline"
								class="h-11 flex-1 border-primary bg-secondary text-xs font-bold text-secondary-foreground"
								loading={pendingUserId === user.id}
								disabled={pendingUserId !== null}
								onclick={() => toggleFrozen(user)}
							>
								{m.admin_users_unfreeze_button()}
							</LoadingButton>
							<!-- 削除は凍結中のユーザーにだけ出す。予約済みなら
							     押しても予定日が動かないだけなので、出さない。 -->
							{#if !user.deletionScheduledAt}
								<Button
									variant="ghost"
									size="icon"
									class="size-11 shrink-0 bg-destructive/10 text-destructive hover:bg-destructive/20"
									aria-label={m.admin_users_delete_label({ name })}
									onclick={() => deleteDialog?.show(user)}
								>
									<Trash2Icon size={16} strokeWidth={2.2} aria-hidden="true" />
								</Button>
							{/if}
						{:else}
							<LoadingButton
								variant="outline"
								class="h-11 flex-1 text-xs font-bold text-muted-foreground"
								loading={pendingUserId === user.id}
								disabled={blocked || pendingUserId !== null}
								onclick={() => toggleFrozen(user)}
							>
								{m.admin_users_freeze_button()}
							</LoadingButton>
							<!-- 押しても 409 になる操作は並べない。自分自身は設定画面から変更する経路が
							     あり、Google 専用アカウントはパスワードを持たない。 -->
							{#if user.passwordUsable && !isSelf(user)}
								<Button
									variant="outline"
									class="h-11 flex-1 text-xs font-bold text-muted-foreground"
									onclick={() => resetPasswordDialog?.show(user)}
								>
									{m.admin_users_reset_password_button()}
								</Button>
							{/if}
						{/if}
					</div>
					{#if user.deletionScheduledAt}
						<div class="mt-2 flex items-center justify-between gap-2">
							<p class="text-xs text-destructive">
								{m.admin_users_deletion_scheduled({
									date: formatFutureDateLabel(new Date(user.deletionScheduledAt), userTimeZone())
								})}
							</p>
							<!-- デザインには無いボタン (docs/authentication.md)。取り消せるのは
							     管理者が起点の予約だけで、本人の申し出は本人がログインして取り消す。 -->
							{#if user.deletionOrigin === 'admin'}
								<LoadingButton
									variant="outline"
									class="h-11 shrink-0 text-xs font-bold text-muted-foreground"
									loading={pendingUserId === user.id}
									disabled={pendingUserId !== null}
									onclick={() => cancelDeletion(user)}
								>
									{m.admin_users_cancel_deletion_button()}
								</LoadingButton>
							{/if}
						</div>
					{/if}
					<!-- 無効なボタンは読み上げの焦点が当たらないため、理由は文字として出す。 -->
					{#if blocked}
						<p class="mt-2 text-xs text-muted-foreground">{m.admin_users_self_freeze_hint()}</p>
					{/if}
					<!-- ボタンが出ないだけだと、変更する手段が無いように見える。 -->
					{#if isSelf(user) && user.passwordUsable}
						<p class="mt-2 text-xs text-muted-foreground">
							{m.admin_users_self_reset_password_hint()}
						</p>
					{/if}
				</li>
			{/each}
		</ul>
	{/if}
</div>

<AdminOcrLimitDialog bind:this={ocrLimitDialog} onsaved={load} />
<AdminResetPasswordDialog bind:this={resetPasswordDialog} onreset={load} />
<AdminDeleteUserDialog bind:this={deleteDialog} onscheduled={load} />
<AdminAddUserDialog bind:this={addDialog} onadded={load} />
<NoticeDialog bind:this={errorDialog} />
