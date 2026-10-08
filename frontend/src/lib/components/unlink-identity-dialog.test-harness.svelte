<script lang="ts">
	import type { LinkedProvider } from '$lib/auth';
	import UnlinkIdentityDialog from '$lib/components/unlink-identity-dialog.svelte';

	/** テスト用: ボタンで開く。 */
	let { provider }: { provider: LinkedProvider } = $props();
	let dialog = $state<ReturnType<typeof UnlinkIdentityDialog> | null>(null);
</script>

<button type="button" onclick={() => dialog?.show(provider)}>open</button>
<UnlinkIdentityDialog bind:this={dialog} onunlinked={() => {}} />
