<script lang="ts">
	// ADR: 角丸は shadcn-svelte 既定の rounded-xl から rounded-lg に改変。layout.css の角丸トークンはデザインの実寸で、モーダル・カードは radius-lg (20px)。
	// ADR: shadcn-svelte 既定の右上の閉じるボタン (28px) と `showCloseButton` を取り除く改変。タップ領域の基準に届かず、
	// × を出すのはフォームのダイアログだけと決めたため。× は見出しに置く (`dialog-form-header.svelte`)。
	// ADR: 幅と余白の改変 (shadcn-svelte 既定は max-w-[calc(100%-2rem)]・sm:max-w-sm・p-4)。デザインのダイアログは画面の端から 24px 内側に置き、中の余白も 24px。sm 以上の幅は記録フォームのシートと同じ max-w-md (文言の長いボタンを同じ幅で並べて収めるため)。
	import { Dialog as DialogPrimitive } from "bits-ui";
	import { cn, type WithoutChildrenOrChild } from "$lib/utils.js";
	import * as Dialog from "./index.js";
	import DialogPortal from "./dialog-portal.svelte";
	import type { Snippet } from "svelte";
	import type { ComponentProps } from "svelte";

	let {
		ref = $bindable(null),
		class: className,
		portalProps,
		children,
		...restProps
	}: WithoutChildrenOrChild<DialogPrimitive.ContentProps> & {
		portalProps?: WithoutChildrenOrChild<ComponentProps<typeof DialogPortal>>;
		children: Snippet;
	} = $props();
</script>

<DialogPortal {...portalProps}>
	<Dialog.Overlay />
	<DialogPrimitive.Content
		bind:ref
		data-slot="dialog-content"
		class={cn(
			"grid max-w-[calc(100%-3rem)] gap-4 rounded-lg bg-popover p-6 text-sm text-popover-foreground ring-1 ring-foreground/10 duration-100 sm:max-w-md data-open:animate-in data-open:fade-in-0 data-open:zoom-in-95 data-closed:animate-out data-closed:fade-out-0 data-closed:zoom-out-95 fixed top-1/2 left-1/2 z-50 w-full -translate-x-1/2 -translate-y-1/2 outline-none",
			className
		)}
		{...restProps}
	>
		{@render children?.()}
	</DialogPrimitive.Content>
</DialogPortal>
