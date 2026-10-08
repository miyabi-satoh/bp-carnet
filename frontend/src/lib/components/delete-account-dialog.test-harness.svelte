<script lang="ts">
	import DeleteAccountDialog from '$lib/components/delete-account-dialog.svelte';

	/** テスト用: ボタンで開く。 */
	let dialog = $state<ReturnType<typeof DeleteAccountDialog> | null>(null);
</script>

<button type="button" onclick={() => dialog?.show()}>open</button>
<DeleteAccountDialog bind:this={dialog} />
