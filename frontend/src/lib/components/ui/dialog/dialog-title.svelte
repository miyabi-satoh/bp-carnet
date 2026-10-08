<script lang="ts">
	// ADR: 見出しの文字を shadcn-svelte 既定の text-base font-medium から text-lg font-extrabold に改変。ダイアログの見出しはどの種類も太字にそろえる。
	import { Dialog as DialogPrimitive } from "bits-ui";
	import { cn } from "$lib/utils.js";

	let {
		ref = $bindable(null),
		class: className,
		...restProps
	}: DialogPrimitive.TitleProps = $props();
</script>

<DialogPrimitive.Title
	bind:ref
	data-slot="dialog-title"
	class={cn("text-lg leading-tight font-extrabold", className)}
	{...restProps}
/>
