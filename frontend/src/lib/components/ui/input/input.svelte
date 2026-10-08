<script lang="ts">
	// ADR: 角丸は shadcn-svelte 既定の rounded-lg から rounded-sm に改変。layout.css の角丸トークンはデザインの実寸で、入力欄は radius-sm (10px)。
	// ADR: 地の色は shadcn-svelte 既定の bg-transparent / dark:bg-input/30 から bg-field (layout.css) に改変。デザインの入力欄は、ライトは白、ダークはカードより少し明るい地。
	import { cn, type WithElementRef } from "$lib/utils.js";
	import type { HTMLInputAttributes, HTMLInputTypeAttribute } from "svelte/elements";

	type InputType = Exclude<HTMLInputTypeAttribute, "file">;

	type Props = WithElementRef<
		Omit<HTMLInputAttributes, "type"> &
			({ type: "file"; files?: FileList } | { type?: InputType; files?: undefined })
	>;

	let {
		ref = $bindable(null),
		value = $bindable(),
		type,
		files = $bindable(),
		class: className,
		"data-slot": dataSlot = "input",
		...restProps
	}: Props = $props();
</script>

{#if type === "file"}
	<input
		bind:this={ref}
		data-slot={dataSlot}
		class={cn(
		// ADR: 高さは shadcn-svelte 既定 (h-8=32px) からの改変。
			"h-11 rounded-sm border border-input bg-field px-2.5 py-1 text-base transition-colors file:h-6 file:text-sm file:font-medium focus-visible:border-ring focus-visible:ring-3 focus-visible:ring-ring/50 disabled:bg-input/50 aria-invalid:border-destructive aria-invalid:ring-3 aria-invalid:ring-destructive/20 md:text-sm dark:disabled:bg-input/80 dark:aria-invalid:border-destructive/50 dark:aria-invalid:ring-destructive/40 w-full min-w-0 outline-none file:inline-flex file:border-0 file:bg-transparent file:text-foreground placeholder:text-muted-foreground disabled:pointer-events-none disabled:cursor-not-allowed disabled:opacity-50",
			className
		)}
		type="file"
		bind:files
		bind:value
		{...restProps}
	/>
{:else}
	<input
		bind:this={ref}
		data-slot={dataSlot}
		class={cn(
		// ADR: 高さは shadcn-svelte 既定 (h-8=32px) からの改変。
			"h-11 rounded-sm border border-input bg-field px-2.5 py-1 text-base transition-colors file:h-6 file:text-sm file:font-medium focus-visible:border-ring focus-visible:ring-3 focus-visible:ring-ring/50 disabled:bg-input/50 aria-invalid:border-destructive aria-invalid:ring-3 aria-invalid:ring-destructive/20 md:text-sm dark:disabled:bg-input/80 dark:aria-invalid:border-destructive/50 dark:aria-invalid:ring-destructive/40 w-full min-w-0 outline-none file:inline-flex file:border-0 file:bg-transparent file:text-foreground placeholder:text-muted-foreground disabled:pointer-events-none disabled:cursor-not-allowed disabled:opacity-50",
			// ADR: 日付・時刻の欄は、iOS の WebKit が独自の見た目 (appearance) で h-11 より高く・幅を超えて描くため、iOS でだけ外す。shadcn-svelte に無い改変。
			(type === "date" || type === "time") && "supports-[-webkit-touch-callout:none]:appearance-none",
			className
		)}
		{type}
		bind:value
		{...restProps}
	/>
{/if}
