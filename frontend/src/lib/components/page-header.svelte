<script lang="ts">
	import type { Snippet } from 'svelte';
	import RoundButton from '$lib/components/round-button.svelte';
	import ChevronLeftIcon from '@lucide/svelte/icons/chevron-left';
	import * as m from '$lib/paraglide/messages.js';

	/** 共通ヘッダーの代わりに置く、丸い戻るボタンと見出し。
	 * ページの上端に置く前提で、上の余白に画面の上端の帯を含める。
	 * 右側に操作を置くときは `actions` で渡す。 */
	let {
		backHref,
		onback,
		backDisabled,
		backLabel = m.common_back_button(),
		title,
		heading = $bindable(null),
		actions
	}: {
		/** `resolve()` 済みの戻り先。渡さなければ戻るボタンはボタンになり、`onback` を呼ぶ。 */
		backHref?: string;
		/** `backHref` を渡さないときの、戻るボタンを押したときの処理。 */
		onback?: () => void;
		/** 戻るボタンを押せなくするか (`backHref` を渡さないとき)。 */
		backDisabled?: boolean;
		/** 戻るボタンの読み上げ名。既定は「戻る」。 */
		backLabel?: string;
		title: string;
		/** 見出しの要素。操作の後にフォーカスを移す先にする。 */
		heading?: HTMLElement | null;
		actions?: Snippet;
	} = $props();
</script>

<div class="flex items-center justify-between gap-3 pt-safe-5 pb-5">
	<div class="flex min-w-0 items-center gap-3">
		<RoundButton
			href={backHref}
			class="raised bg-card"
			aria-label={backLabel}
			disabled={backDisabled}
			onclick={onback}
		>
			<ChevronLeftIcon size={18} strokeWidth={2.2} aria-hidden="true" />
		</RoundButton>
		<h1
			bind:this={heading}
			tabindex="-1"
			class="min-w-0 truncate text-lg font-extrabold outline-none"
		>
			{title}
		</h1>
	</div>
	{@render actions?.()}
</div>
