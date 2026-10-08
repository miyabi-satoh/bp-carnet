<script lang="ts">
	import Loader2Icon from '@lucide/svelte/icons/loader-2';
	import { cn } from "$lib/utils.js";
	import type { SVGAttributes } from "svelte/elements";

	let {
		class: className,
		role = "status",
		// we add name, color, and stroke for compatibility with different icon libraries props
		name,
		color,
		stroke,
		"aria-label": ariaLabel = "Loading",
		...restProps
	}: SVGAttributes<SVGSVGElement> = $props();

	// FIX: @lucide/svelte の Icon は受け取った属性を既定の属性 (stroke="currentColor") の上に重ねるため、
	// undefined を渡すと線の色が消える。未指定のものは属性ごと渡さない。
	const iconProps = $derived({
		...(name == null ? {} : { name }),
		...(color == null ? {} : { color }),
		...(stroke == null ? {} : { stroke })
	});
</script>

<Loader2Icon {role} {...iconProps} aria-label={ariaLabel} class={cn("size-4 animate-spin", className)} {...restProps} />
