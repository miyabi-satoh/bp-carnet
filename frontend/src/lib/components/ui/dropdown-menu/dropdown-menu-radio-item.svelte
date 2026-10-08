<script lang="ts">
	// ADR: 角丸は shadcn-svelte 既定から一段小さく改変。layout.css の角丸トークンがデザインの実寸 (sm/md/lg = 10/14/20px) で、既定のままだと丸すぎるため。
	// ADR: shadcn-svelte 既定の `cursor-default` を外す改変。押せるものはカーソルを指さしに
	// そろえる決まりを layout.css で当てており、それを打ち消さないため。
	import { DropdownMenu as DropdownMenuPrimitive } from "bits-ui";
	import CheckIcon from '@lucide/svelte/icons/check';
	import { cn, type WithoutChild } from "$lib/utils.js";

	let {
		ref = $bindable(null),
		class: className,
		children: childrenProp,
		...restProps
	}: WithoutChild<DropdownMenuPrimitive.RadioItemProps> = $props();
</script>

<DropdownMenuPrimitive.RadioItem
	bind:ref
	data-slot="dropdown-menu-radio-item"
	class={cn(
		// dropdown-menu-item.svelte と同じ理由で min-h-11 を足す (mode-toggle のテーマ選択にも影響)。
		"min-h-11 gap-1.5 rounded-sm py-1 pr-8 pl-1.5 text-sm focus:bg-accent focus:text-accent-foreground focus:**:text-accent-foreground data-inset:pl-7 [&_svg:not([class*='size-'])]:size-4 relative flex items-center outline-hidden select-none data-[disabled]:pointer-events-none data-[disabled]:opacity-50 [&_svg]:pointer-events-none [&_svg]:shrink-0",
		className
	)}
	{...restProps}
>
	{#snippet children({ checked })}
		<span
			class="absolute right-2 flex items-center justify-center pointer-events-none"
			data-slot="dropdown-menu-radio-item-indicator"
		>
			{#if checked}
				<CheckIcon  />
			{/if}
		</span>
		{@render childrenProp?.({ checked })}
	{/snippet}
</DropdownMenuPrimitive.RadioItem>
