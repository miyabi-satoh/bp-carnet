<script lang="ts">
	// ADR: 欄の間隔は shadcn-svelte 既定 (gap-5) から gap-4 に改変。デザインは縦に並べた欄の間を 16px にしている。
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
	data-slot="field-group"
	class={cn(
		"gap-4 data-[slot=checkbox-group]:gap-3 *:data-[slot=field-group]:gap-4 group/field-group @container/field-group flex w-full flex-col",
		className
	)}
	{...restProps}
>
	{@render children?.()}
</div>
