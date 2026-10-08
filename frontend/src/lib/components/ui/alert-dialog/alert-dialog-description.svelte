<script lang="ts">
	// ADR: shadcn-svelte 既定の text-balance・md:text-pretty を外す改変。日本語の文が行の半ばで折り返され、Dialog.Description と見た目がずれるため。
	import { AlertDialog as AlertDialogPrimitive } from "bits-ui";
	import { cn } from "$lib/utils.js";

	let {
		ref = $bindable(null),
		class: className,
		...restProps
	}: AlertDialogPrimitive.DescriptionProps = $props();
</script>

<AlertDialogPrimitive.Description
	bind:ref
	data-slot="alert-dialog-description"
	class={cn("text-sm text-muted-foreground *:[a]:underline *:[a]:underline-offset-3 *:[a]:hover:text-foreground", className)}
	{...restProps}
/>
