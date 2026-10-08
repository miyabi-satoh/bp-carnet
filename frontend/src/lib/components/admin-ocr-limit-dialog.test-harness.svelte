<script lang="ts">
	import type { AdminUser } from '$lib/admin';
	import AdminOcrLimitDialog from '$lib/components/admin-ocr-limit-dialog.svelte';

	/** テスト用: ボタンで対象ユーザーを渡して開き、保存の通知の回数を表示する。 */
	let { user }: { user: AdminUser } = $props();

	let dialog = $state<ReturnType<typeof AdminOcrLimitDialog> | null>(null);
	let savedCount = $state(0);
</script>

<button type="button" onclick={() => dialog?.show(user, 80)}>open</button>
<AdminOcrLimitDialog bind:this={dialog} onsaved={() => (savedCount += 1)} />
<p data-testid="saved-count">{savedCount}</p>
