<script lang="ts">
	// ADR: 角丸は shadcn-svelte 既定から一段小さく改変。layout.css の角丸トークンがデザインの実寸 (sm/md/lg = 10/14/20px) で、既定のままだと丸すぎるため。
	// ADR: shadcn-svelte 既定の `cursor-default` を外す改変。押せるものはカーソルを指さしに
	// そろえる決まりを layout.css で当てており、それを打ち消さないため。
	import { DropdownMenu as DropdownMenuPrimitive } from "bits-ui";
	import ChevronRightIcon from '@lucide/svelte/icons/chevron-right';
	import { cn } from "$lib/utils.js";

	let {
		ref = $bindable(null),
		class: className,
		inset,
		children,
		...restProps
	}: DropdownMenuPrimitive.SubTriggerProps & {
		inset?: boolean;
	} = $props();
</script>

<DropdownMenuPrimitive.SubTrigger
	bind:ref
	data-slot="dropdown-menu-sub-trigger"
	data-inset={inset}
	class={cn(
		"gap-1.5 rounded-sm px-1.5 py-1 text-sm focus:bg-accent focus:text-accent-foreground not-data-[variant=destructive]:focus:**:text-accent-foreground data-inset:pl-7 data-open:bg-accent data-open:text-accent-foreground [&_svg:not([class*='size-'])]:size-4 flex items-center outline-hidden select-none data-[inset]:pl-8 [&_svg]:pointer-events-none [&_svg]:shrink-0",
		className
	)}
	{...restProps}
>
	{@render children?.()}
	<ChevronRightIcon class="ml-auto" />
</DropdownMenuPrimitive.SubTrigger>
