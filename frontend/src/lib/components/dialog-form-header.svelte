<script lang="ts">
	import DialogRoundClose from '$lib/components/dialog-round-close.svelte';
	import * as Dialog from '$lib/components/ui/dialog';

	/** フォームのダイアログの見出し。見出しの右に丸い × を置く。
	 * × を出すのはフォームのダイアログだけなので、確認のダイアログはこれを使わず `Dialog.Header` を置く。 */
	let {
		title,
		description,
		closeDisabled = false,
		class: className
	}: {
		title: string;
		description?: string;
		/** 送信中は閉じさせない。 */
		closeDisabled?: boolean;
		class?: string;
	} = $props();
</script>

<Dialog.Header class={className}>
	<div class="flex items-center justify-between gap-3">
		<Dialog.Title>{title}</Dialog.Title>
		<!-- 見出しの1行目と × の高さを合わせるため、× の上下の余白を見出しの側に食い込ませる。 -->
		<DialogRoundClose disabled={closeDisabled} class="-my-2 -mr-2 bg-background dark:bg-muted" />
	</div>
	{#if description}
		<Dialog.Description>{description}</Dialog.Description>
	{/if}
</Dialog.Header>
