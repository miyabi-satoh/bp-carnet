<script lang="ts">
	// ADR: shadcn-svelte 既定の、横並びの Field での text-balance を外す改変。日本語の文が右端を空けたまま語の途中で折り返されるため。
	import { cn, type WithElementRef } from "$lib/utils.js";
	import type { HTMLAttributes } from "svelte/elements";

	let {
		ref = $bindable(null),
		class: className,
		children,
		...restProps
	}: WithElementRef<HTMLAttributes<HTMLParagraphElement>> = $props();
</script>

<p
	bind:this={ref}
	data-slot="field-description"
	class={cn(
		"text-left text-sm text-muted-foreground [[data-variant=legend]+&]:-mt-1.5 leading-normal font-normal",
		"last:mt-0 nth-last-2:-mt-1",
		"[&>a]:underline [&>a]:underline-offset-4 [&>a:hover]:text-primary",
		className
	)}
	{...restProps}
>
	{@render children?.()}
</p>
