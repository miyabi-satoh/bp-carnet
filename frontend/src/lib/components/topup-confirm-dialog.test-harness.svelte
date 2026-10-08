<script lang="ts">
	import TopupConfirmDialog from '$lib/components/topup-confirm-dialog.svelte';

	let { onpurchased }: { onpurchased?: () => void } = $props();

	/** テスト用: ボタンで開く。 */
	let dialog = $state<ReturnType<typeof TopupConfirmDialog> | null>(null);
</script>

<button type="button" onclick={() => dialog?.show()}>open</button>
<TopupConfirmDialog bind:this={dialog} {onpurchased} />
