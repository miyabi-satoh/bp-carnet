<script lang="ts">
	import ChevronLeftIcon from '@lucide/svelte/icons/chevron-left';
	import XIcon from '@lucide/svelte/icons/x';
	import RoundButton from '$lib/components/round-button.svelte';
	import * as Dialog from '$lib/components/ui/dialog';
	import * as m from '$lib/paraglide/messages.js';

	/** ダイアログの見出しに置く丸い閉じるボタン。shadcn の Dialog.Content 既定の閉じるボタン
	 * (28px) はタップ領域の基準に届かないため、こちらを使う。地の色は置き場所で変わるので `class` で渡す。 */
	let {
		icon = 'close',
		disabled = false,
		class: className
	}: {
		/** 前の画面へ戻る位置づけの画面は`back`。どちらもダイアログを閉じる。 */
		icon?: 'close' | 'back';
		disabled?: boolean;
		class?: string;
	} = $props();
</script>

<Dialog.Close {disabled}>
	{#snippet child({ props })}
		<RoundButton {...props} class={className}>
			{#if icon === 'back'}
				<ChevronLeftIcon size={18} strokeWidth={2.2} aria-hidden="true" />
			{:else}
				<XIcon size={18} strokeWidth={2.2} aria-hidden="true" />
			{/if}
			<span class="sr-only">{m.common_close_button()}</span>
		</RoundButton>
	{/snippet}
</Dialog.Close>
