<script lang="ts">
	import { Spinner } from '$lib/components/ui/spinner';
	import * as m from '$lib/paraglide/messages.js';
	import { cn } from '$lib/utils';

	/** 応答待ちの表示 (スピナー + 文言)。画面・ダイアログ本文の読み込み待ちに使う (ボタンの
	 * 送信待ちは `loading-button.svelte`)。
	 *
	 * スピナーは `aria-hidden` にし、`role="status"` を付けた文言だけを読み上げさせる
	 * (スピナー側の既定の `aria-label` と二重に読まれるのを防ぐ)。 */
	let { label = m.common_loading(), class: className }: { label?: string; class?: string } =
		$props();
</script>

<p
	role="status"
	class={cn('flex items-center justify-center gap-2 py-8 text-sm text-muted-foreground', className)}
>
	<Spinner aria-hidden="true" />
	{label}
</p>
