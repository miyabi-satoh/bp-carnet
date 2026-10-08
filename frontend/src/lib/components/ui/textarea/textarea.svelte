<script lang="ts">
	// ADR: 角丸は shadcn-svelte 既定の rounded-lg から rounded-sm に改変。layout.css の角丸トークンはデザインの実寸で、入力欄は radius-sm (10px)。
	// ADR: 地の色は shadcn-svelte 既定の bg-transparent / dark:bg-input/30 から bg-field (layout.css) に改変。デザインの入力欄は、ライトは白、ダークはカードより少し明るい地。
	import { cn, type WithElementRef, type WithoutChildren } from "$lib/utils.js";
	import type { HTMLTextareaAttributes } from "svelte/elements";

	let {
		ref = $bindable(null),
		value = $bindable(),
		class: className,
		"data-slot": dataSlot = "textarea",
		...restProps
	}: WithoutChildren<WithElementRef<HTMLTextareaAttributes>> = $props();
</script>

<textarea
	bind:this={ref}
	data-slot={dataSlot}
	class={cn(
		"rounded-sm border border-input bg-field px-2.5 py-2 text-base transition-colors focus-visible:border-ring focus-visible:ring-3 focus-visible:ring-ring/50 disabled:bg-input/50 aria-invalid:border-destructive aria-invalid:ring-3 aria-invalid:ring-destructive/20 md:text-sm dark:disabled:bg-input/80 dark:aria-invalid:border-destructive/50 dark:aria-invalid:ring-destructive/40 flex field-sizing-content min-h-16 w-full outline-none placeholder:text-muted-foreground disabled:cursor-not-allowed disabled:opacity-50",
		className
	)}
	bind:value
	{...restProps}></textarea>
