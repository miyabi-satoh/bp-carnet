<script lang="ts">
	import { onMount } from 'svelte';
	import { page } from '$app/state';
	import { resolve } from '$app/paths';
	import {
		adminUserName,
		adminUserStatusLabel,
		fetchAdminUser,
		fetchAdminUserRecords,
		type AdminUserDetail
	} from '$lib/admin';
	import { LINKED_PROVIDERS, linkedProviderLabel, userTimeZone } from '$lib/auth';
	import * as m from '$lib/paraglide/messages.js';
	import {
		formatDateLabel,
		formatDateOnly,
		localToday,
		minutesToTimeText,
		periodFor
	} from '$lib/period';
	import { RecordsViewState } from '$lib/records-view.svelte';
	import { timezoneLabel } from '$lib/settings';
	import { SECTION_TITLE_CLASS } from '$lib/styles';
	import LoadError from '$lib/components/load-error.svelte';
	import LoadingIndicator from '$lib/components/loading-indicator.svelte';
	import PageHeader from '$lib/components/page-header.svelte';
	import PeriodNav from '$lib/components/period-nav.svelte';
	import RecordsOverview from '$lib/components/records-overview.svelte';

	/** 管理者による利用者のアカウント情報・血圧記録の閲覧 (docs/authentication.md)。読み取り専用で、
	 * 記録の追加・編集の導線は置かない。id の形式は `+page.ts` で確かめ済み。 */
	const userId = Number(page.params.id);

	let user = $state<AdminUserDetail | null>(null);
	let loadErrorMessage = $state('');

	const recordsView = new RecordsViewState((from, to) => fetchAdminUserRecords(userId, from, to));

	function loginMethodLabel(user: AdminUserDetail): string {
		const linked = { line: user.lineLinked, google: user.googleLinked, apple: user.appleLinked };
		return [
			...(user.passwordUsable ? [m.admin_user_detail_login_method_password()] : []),
			...LINKED_PROVIDERS.filter((p) => linked[p]).map(linkedProviderLabel)
		].join(m.common_list_separator());
	}

	function periodRangeLabel(startMin: number, endMin: number): string {
		return `${minutesToTimeText(startMin)} ${m.common_range_separator()} ${minutesToTimeText(endMin)}`;
	}

	let accountRows = $derived(
		user
			? [
					{ label: m.common_username_label(), value: user.username },
					{ label: m.admin_user_detail_status_label(), value: adminUserStatusLabel(user) },
					{
						label: m.admin_user_detail_created_at_label(),
						value: formatDateLabel(new Date(user.createdAt), true, userTimeZone())
					},
					{ label: m.admin_user_detail_login_method_label(), value: loginMethodLabel(user) },
					{ label: m.admin_user_detail_timezone_label(), value: timezoneLabel(user.timezone) },
					{
						label: m.admin_user_detail_period_label({
							period: m.record_day_period_morning_label()
						}),
						value: periodRangeLabel(user.morningStartMin, user.morningEndMin)
					},
					{
						label: m.admin_user_detail_period_label({
							period: m.record_day_period_evening_label()
						}),
						value: periodRangeLabel(user.eveningStartMin, user.eveningEndMin)
					}
				]
			: []
	);

	async function loadUser() {
		loadErrorMessage = '';
		const result = await fetchAdminUser(userId);
		if (!result.ok) {
			loadErrorMessage = result.message;
			return;
		}
		// 「今週」は対象ユーザーのタイムゾーンで決める。記録の日付もそのタイムゾーンで区切られるため。
		const today = localToday(result.user.timezone);
		const initialPeriod = periodFor('week', today, today);
		recordsView.from = formatDateOnly(initialPeriod.from);
		recordsView.to = formatDateOnly(initialPeriod.to);
		user = result.user;
		recordsView.load();
	}

	onMount(loadUser);
</script>

<!-- デザインに無い画面。ユーザー管理と同じく、共通ヘッダーの代わりに
     丸い戻るボタンと見出しを置き、記録の部分はメイン画面の並びに揃える。 -->
<div class="mx-auto flex max-w-2xl flex-col px-5 pb-safe-5">
	<PageHeader
		backHref={resolve('/admin/users')}
		backLabel={m.admin_user_detail_back_label()}
		title={user ? adminUserName(user) : m.admin_users_title()}
	/>

	{#if loadErrorMessage}
		<LoadError message={loadErrorMessage} onretry={loadUser} />
	{:else if user === null}
		<LoadingIndicator />
	{:else}
		<div class="flex flex-col gap-4">
			<p class="text-xs text-muted-foreground">{m.admin_user_detail_audit_notice()}</p>

			<section aria-labelledby="account-heading">
				<h2 id="account-heading" class={SECTION_TITLE_CLASS}>
					{m.admin_user_detail_account_section_title()}
				</h2>
				<dl class="divide-y rounded-lg raised bg-card">
					{#each accountRows as row (row.label)}
						<div class="flex min-h-13 items-center justify-between gap-3 px-4 py-3">
							<dt class="shrink-0 text-sm text-muted-foreground">{row.label}</dt>
							<dd class="min-w-0 text-right text-base break-all">{row.value}</dd>
						</div>
					{/each}
				</dl>
			</section>

			<section aria-labelledby="records-heading">
				<h2 id="records-heading" class={SECTION_TITLE_CLASS}>
					{m.admin_user_detail_records_section_title()}
				</h2>
				<div class="flex flex-col gap-4">
					<PeriodNav
						from={recordsView.from}
						to={recordsView.to}
						timeZone={user.timezone}
						loading={recordsView.loading}
						onchange={recordsView.changePeriod}
					/>
					<RecordsOverview
						records={recordsView.records}
						timeZone={user.timezone}
						summary={recordsView.summary}
						loading={recordsView.loading}
						errorMessage={recordsView.errorMessage}
						onretry={recordsView.load}
						bind:visibleKeys={recordsView.visibleSeriesKeys}
						bind:listLimit={recordsView.listLimit}
					/>
				</div>
			</section>
		</div>
	{/if}
</div>
