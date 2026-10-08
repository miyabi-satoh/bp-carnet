<script lang="ts">
	// ADR: 角丸は shadcn-svelte 既定の rounded-xl から rounded-lg に改変。layout.css の角丸トークンはデザインの実寸で、モーダル・カードは radius-lg (20px)。
	import { cn, type WithElementRef } from "$lib/utils.js";
	import type { HTMLAttributes } from "svelte/elements";

	let {
		ref = $bindable(null),
		class: className,
		children,
		...restProps
	}: WithElementRef<HTMLAttributes<HTMLDivElement>> = $props();
</script>

<div
	bind:this={ref}
	data-slot="card-footer"
	class={cn("rounded-b-lg border-t bg-muted/50 p-(--card-spacing) flex items-center", className)}
	{...restProps}
>
	{@render children?.()}
</div>
