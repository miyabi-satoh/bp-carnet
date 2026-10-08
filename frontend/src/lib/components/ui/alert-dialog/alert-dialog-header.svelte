<script lang="ts">
	// ADR: 見出しの並びを shadcn-svelte 既定の中央寄せ (モバイル) から、Dialog.Header と同じ左寄せの縦並びに改変。ダイアログの種類によらず見出しの位置をそろえる。
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
	data-slot="alert-dialog-header"
	class={cn("gap-2 flex flex-col", className)}
	{...restProps}
>
	{@render children?.()}
</div>
