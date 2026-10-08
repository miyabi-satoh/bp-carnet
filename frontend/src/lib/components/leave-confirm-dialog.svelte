<script lang="ts">
	import type { LeaveGuard } from '$lib/leave-guard.svelte';
	import * as AlertDialog from '$lib/components/ui/alert-dialog';
	import * as m from '$lib/paraglide/messages.js';

	/** 入力の途中でページを離れる・入力を捨てる前の確認 (`LeaveGuard`)。文言は、ページを離れるとき用が既定。 */
	let {
		guard,
		title = m.leave_confirm_dialog_title(),
		leaveLabel = m.leave_confirm_dialog_leave_button()
	}: {
		guard: LeaveGuard;
		title?: string;
		leaveLabel?: string;
	} = $props();
</script>

<AlertDialog.Root
	bind:open={
		() => guard.dialogOpen,
		(open) => {
			if (!open) guard.stay();
		}
	}
>
	<AlertDialog.Content>
		<AlertDialog.Header>
			<AlertDialog.Title>{title}</AlertDialog.Title>
			<AlertDialog.Description>{m.leave_confirm_dialog_description()}</AlertDialog.Description>
		</AlertDialog.Header>
		<AlertDialog.Footer>
			<AlertDialog.Cancel>{m.leave_confirm_dialog_stay_button()}</AlertDialog.Cancel>
			<AlertDialog.Action onclick={() => guard.leave()}>
				{leaveLabel}
			</AlertDialog.Action>
		</AlertDialog.Footer>
	</AlertDialog.Content>
</AlertDialog.Root>
