<script lang="ts">
	import { Button, type ButtonProps } from '$lib/components/ui/button';
	import { Spinner } from '$lib/components/ui/spinner';
	import * as m from '$lib/paraglide/messages.js';
	import { cn } from '$lib/utils';

	/** 送信中はスピナーを出すボタン。
	 *
	 * スピナーは中身の中央に重ね、中身は場所を取ったまま透明にする。横に足すとボタンの幅が
	 * 広がり、幅の決まったボタンでは文字の位置がずれるため。
	 *
	 * 透明にするのは `opacity-0` で、`invisible` (`visibility: hidden`) は使わない。後者だと
	 * 中身が読み上げから外れ、送信中のボタンが「読み込み中」としか読まれなくなるため。
	 * 送信中であることは `aria-busy` とスピナーの `aria-label` で伝える。 */
	type Props = ButtonProps & {
		loading?: boolean;
	};

	let { loading = false, disabled, children, class: className, ...restProps }: Props = $props();
</script>

<Button
	disabled={loading || disabled}
	aria-busy={loading}
	class={cn('relative', className)}
	{...restProps}
>
	<!-- 中身の並べ方は Button に合わせる。`gap` は size ごとに違うので継承させる。 -->
	<span class={['inline-flex items-center gap-[inherit]', loading && 'opacity-0']}>
		{@render children?.()}
	</span>
	{#if loading}
		<span class="absolute inset-0 flex items-center justify-center">
			<Spinner aria-label={m.loading_button_spinner_label()} />
		</span>
	{/if}
</Button>
