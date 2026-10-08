<script lang="ts">
	// ADR: 角丸は shadcn-svelte 既定から一段小さく改変。layout.css の角丸トークンがデザインの実寸 (sm/md/lg = 10/14/20px) で、既定のままだと丸すぎるため。
	// ADR: shadcn-svelte 既定の `cursor-default` を外す改変。押せるものはカーソルを指さしに
	// そろえる決まりを layout.css で当てており、それを打ち消さないため。
	import { DropdownMenu as DropdownMenuPrimitive } from "bits-ui";
	import { cn } from "$lib/utils.js";

	let {
		ref = $bindable(null),
		class: className,
		inset,
		variant = "default",
		...restProps
	}: DropdownMenuPrimitive.ItemProps & {
		inset?: boolean;
		variant?: "default" | "destructive";
	} = $props();
</script>

<DropdownMenuPrimitive.Item
	bind:ref
	data-slot="dropdown-menu-item"
	data-inset={inset}
	data-variant={variant}
	class={cn(
		// shadcn-svelte 既定 (py-1、実質28px程度) はタップ領域の基準 (44px) に
		// 届かないため min-h-11 を足す。
		"min-h-11 gap-1.5 rounded-sm px-1.5 py-1 text-sm focus:bg-accent focus:text-accent-foreground not-data-[variant=destructive]:focus:**:text-accent-foreground data-inset:pl-7 data-[variant=destructive]:text-destructive data-[variant=destructive]:focus:bg-destructive/10 data-[variant=destructive]:focus:text-destructive dark:data-[variant=destructive]:focus:bg-destructive/20 [&_svg:not([class*='size-'])]:size-4 data-[variant=destructive]:*:[svg]:text-destructive group/dropdown-menu-item relative flex items-center outline-hidden select-none data-[inset]:pl-8 data-disabled:pointer-events-none data-disabled:opacity-50 [&_svg]:pointer-events-none [&_svg]:shrink-0",
		className
	)}
	{...restProps}
/>
