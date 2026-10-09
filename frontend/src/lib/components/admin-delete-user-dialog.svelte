<script lang="ts">
	import { scheduleUserDeletion, type AdminUser } from '$lib/admin';
	import { userDisplayName, userTimeZone } from '$lib/auth';
	import TypedConfirmDeleteDialog from '$lib/components/typed-confirm-delete-dialog.svelte';
	import * as m from '$lib/paraglide/messages.js';
	import { formatFutureDateLabel } from '$lib/period';

	type Scheduled = { deletionScheduledAt: string };

	/** ユーザー削除の確認モーダル。一覧のカードから `bind:this` + `show()` で開く
	 * (`delete-account-dialog.svelte` と同じパターン)。 */
	let { onscheduled }: { onscheduled: () => void } = $props();

	let user = $state<AdminUser | null>(null);
	let dialog = $state<ReturnType<typeof TypedConfirmDeleteDialog<Scheduled>> | null>(null);

	export function show(target: AdminUser) {
		user = target;
		dialog?.show();
	}

	function schedule() {
		// `show()` で対象を決めてから開くので、送信できる時点で必ずいる。
		if (!user) throw new Error('admin-delete-user-dialog: submitted without a target user');
		return scheduleUserDeletion(user.id);
	}
</script>

<!-- ADR: 確認文字列は本人削除の「削除する」ではなく対象のユーザーIDにする。他人のアカウントを
	消す操作なので、隣のカードのボタンを誤って押しても入力段階で相手違いに気づけるようにする。 -->
<TypedConfirmDeleteDialog
	bind:this={dialog}
	title={user ? m.admin_users_delete_dialog_title({ name: userDisplayName(user) }) : ''}
	description={m.admin_users_delete_dialog_description()}
	confirmLabel={user ? m.admin_users_delete_confirm_label({ username: user.username }) : ''}
	confirmWord={user?.username ?? ''}
	inputId="delete-user-confirmation"
	submitLabel={m.admin_users_delete_submit_button()}
	errorTitle={m.admin_users_delete_error_title()}
	scheduledTitle={m.admin_users_delete_scheduled_title()}
	scheduledDescription={(result) =>
		m.admin_users_delete_scheduled_description({
			date: formatFutureDateLabel(new Date(result.deletionScheduledAt), userTimeZone())
		})}
	{schedule}
	onscheduled={() => onscheduled()}
/>
