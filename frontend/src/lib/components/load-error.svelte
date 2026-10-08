<script lang="ts">
	import LoadingButton from '$lib/components/loading-button.svelte';
	import * as m from '$lib/paraglide/messages.js';
	import { cn } from '$lib/utils';

	/** 読み込み・送信に失敗した理由の表示。`loading-indicator.svelte` と対になり、応答待ちが
	 * 終わって差し替わる先として使う (入力エラーは `field-error-list.svelte`)。
	 *
	 * 非同期の完了後に現れるため、支援技術へ通知されるよう `role="alert"` を付ける。
	 *
	 * 読み込みの失敗には `onretry` を渡し、「もう一度試す」を出す。ホーム画面に置いたアプリやアプリの中では
	 * ブラウザの再読み込みが無く、ほかにやり直す手段が無いため (docs/mobile-app.md の起動時の画面と揃える)。 */
	let {
		message,
		onretry,
		class: className
	}: { message: string; onretry?: () => Promise<unknown>; class?: string } = $props();

	let retrying = $state(false);

	async function retry() {
		if (!onretry) return;
		retrying = true;
		try {
			await onretry();
		} finally {
			retrying = false;
		}
	}
</script>

<div class={cn('flex flex-col items-start gap-3', className)}>
	<p role="alert" class="text-sm text-destructive">{message}</p>
	{#if onretry}
		<LoadingButton variant="outline" loading={retrying} onclick={retry}>
			{m.common_retry_button()}
		</LoadingButton>
	{/if}
</div>
