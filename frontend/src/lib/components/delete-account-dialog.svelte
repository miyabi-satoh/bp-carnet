<script lang="ts">
	import { invalidateAll } from '$app/navigation';
	import { scheduleAccountDeletion } from '$lib/auth';
	import TypedConfirmDeleteDialog from '$lib/components/typed-confirm-delete-dialog.svelte';
	import * as m from '$lib/paraglide/messages.js';

	/** アカウント削除の確認モーダル。ヘッダーのユーザーメニューと設定画面の両方から開くため、
	 * `logout-dialog.svelte` と同じ `bind:this` + `show()` のパターンにする。 */
	let dialog = $state<ReturnType<typeof TypedConfirmDeleteDialog<object>> | null>(null);

	export function show() {
		dialog?.show();
	}
</script>

<TypedConfirmDeleteDialog
	bind:this={dialog}
	title={m.delete_account_dialog_title()}
	description={m.delete_account_dialog_description()}
	confirmLabel={m.delete_account_dialog_confirm_label()}
	confirmWord={m.delete_account_dialog_confirm_word()}
	inputId="delete-account-confirmation"
	submitLabel={m.delete_account_dialog_submit_button()}
	errorTitle={m.delete_account_dialog_error_title()}
	scheduledTitle={m.delete_account_dialog_scheduled_title()}
	scheduledDescription={() => m.delete_account_dialog_scheduled_description()}
	schedule={scheduleAccountDeletion}
	onscheduled={() => invalidateAll()}
/>
