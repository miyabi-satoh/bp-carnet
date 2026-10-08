<script lang="ts">
	import type { HTMLAnchorAttributes, HTMLButtonAttributes } from 'svelte/elements';
	import { cn } from '$lib/utils';

	/** 44px の丸いアイコンボタン (デザインの戻る・閉じる・期間の前後)。`href` を渡すとリンクになる。
	 * 地の色は置き場所で変わるので `class` で渡す。中身のアイコンと、その読み上げ名は呼び出し側で決める。
	 *
	 * ADR: shadcn の Button は基底で枠線を透明にし、ghost のホバー色も持っていて、この見た目と
	 * ぶつかるため素の要素で組む。 */
	let {
		class: className,
		href,
		type = 'button',
		children,
		...restProps
	}: HTMLButtonAttributes & HTMLAnchorAttributes = $props();

	let classes = $derived(
		cn(
			'inline-flex size-11 shrink-0 items-center justify-center rounded-full text-muted-foreground transition-colors outline-none hover:text-foreground focus-visible:ring-3 focus-visible:ring-ring/50 disabled:pointer-events-none disabled:opacity-50',
			className
		)
	);
</script>

{#if href}
	<!-- `href` は呼び出し側で resolve() 済みのものを受け取る。 -->
	<!-- eslint-disable-next-line svelte/no-navigation-without-resolve -->
	<a class={classes} {href} {...restProps}>
		{@render children?.()}
	</a>
{:else}
	<button class={classes} {type} {...restProps}>
		{@render children?.()}
	</button>
{/if}
