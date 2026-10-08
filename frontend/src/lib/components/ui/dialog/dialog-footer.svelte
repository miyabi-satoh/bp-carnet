<script lang="ts">
	// ADR: 足元の灰色の帯 (区切り線・地の色・はみ出し) を消す改変。スマートフォンの幅で縦積み (実行が上) なのは既定のまま、sm 以上は右寄せではなくボタンを同じ幅で並べる。
	// ADR: 角丸は shadcn-svelte 既定の rounded-xl から rounded-lg に改変。layout.css の角丸トークンはデザインの実寸で、モーダル・カードは radius-lg (20px)。
	// ADR: 閉じるボタンの文言は shadcn-svelte 既定の "Close" から i18n の文言に改変。
	import { Dialog as DialogPrimitive } from "bits-ui";
	import * as m from "$lib/paraglide/messages.js";
	import { Button } from "$lib/components/ui/button/index.js";
	import { cn, type WithElementRef } from "$lib/utils.js";
	import type { HTMLAttributes } from "svelte/elements";

	let {
		ref = $bindable(null),
		class: className,
		children,
		showCloseButton = false,
		...restProps
	}: WithElementRef<HTMLAttributes<HTMLDivElement>> & {
		showCloseButton?: boolean;
	} = $props();
</script>

<div
	bind:this={ref}
	data-slot="dialog-footer"
	class={cn("mt-2 flex flex-col-reverse gap-2.5 sm:flex-row sm:[&>*]:flex-1 sm:[&>*]:basis-0", className)}
	{...restProps}
>
	{@render children?.()}
	{#if showCloseButton}
		<DialogPrimitive.Close>
			{#snippet child({ props })}
				<Button variant="outline" {...props}>{m.common_close_button()}</Button>
			{/snippet}
		</DialogPrimitive.Close>
	{/if}
</div>
