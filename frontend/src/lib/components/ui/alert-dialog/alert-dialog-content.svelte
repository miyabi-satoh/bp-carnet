<script lang="ts">
	// ADR: 幅と余白を Dialog.Content とそろえる改変 (shadcn-svelte 既定は max-w-xs・sm:max-w-sm・p-4)。デザインのダイアログは画面の端から 24px 内側に置き、中の余白も 24px。sm 以上の幅は記録フォームのシートと同じ max-w-md (文言の長いボタンを同じ幅で並べて収めるため)。
	// ADR: 角丸は shadcn-svelte 既定の rounded-xl から rounded-lg に改変。layout.css の角丸トークンはデザインの実寸で、モーダル・カードは radius-lg (20px)。
	import { AlertDialog as AlertDialogPrimitive } from "bits-ui";
	import { cn, type WithoutChild, type WithoutChildrenOrChild } from "$lib/utils.js";
	import AlertDialogOverlay from "./alert-dialog-overlay.svelte";
	import AlertDialogPortal from "./alert-dialog-portal.svelte";
	import type { ComponentProps } from "svelte";

	let {
		ref = $bindable(null),
		class: className,
		size = "default",
		portalProps,
		...restProps
	}: WithoutChild<AlertDialogPrimitive.ContentProps> & {
		size?: "default" | "sm";
		portalProps?: WithoutChildrenOrChild<ComponentProps<typeof AlertDialogPortal>>;
	} = $props();
</script>

<AlertDialogPortal {...portalProps}>
	<AlertDialogOverlay />
	<AlertDialogPrimitive.Content
		bind:ref
		data-slot="alert-dialog-content"
		data-size={size}
		class={cn(
			"gap-4 rounded-lg bg-popover p-6 text-popover-foreground ring-1 ring-foreground/10 duration-100 max-w-[calc(100%-3rem)] sm:max-w-md data-open:animate-in data-open:fade-in-0 data-open:zoom-in-95 data-closed:animate-out data-closed:fade-out-0 data-closed:zoom-out-95 group/alert-dialog-content fixed top-1/2 left-1/2 z-50 grid w-full -translate-x-1/2 -translate-y-1/2 outline-none",
			className
		)}
		{...restProps}
	/>
</AlertDialogPortal>
