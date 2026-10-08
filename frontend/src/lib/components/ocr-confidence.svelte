<script lang="ts">
	import { confidenceLevel, type ConfidenceLevel } from '$lib/ocr';
	import * as m from '$lib/paraglide/messages.js';
	import { cn } from '$lib/utils';

	/** 読み取り信頼度 (「読み取り信頼度: 高」)。帯にするか小さなラベルにするかは、置き場所ごとに `class` で決める。 */
	let { confidence, class: className }: { confidence: number; class?: string } = $props();

	const LABELS: Record<ConfidenceLevel, () => string> = {
		high: m.ocr_confidence_high,
		medium: m.ocr_confidence_medium,
		low: m.ocr_confidence_low
	};

	/** 高はデザインの teal。中・低はデザインに無いため、中は目立たせず、低は確かめるよう促す色にする。 */
	const COLORS: Record<ConfidenceLevel, string> = {
		high: 'bg-secondary text-secondary-foreground',
		medium: 'bg-muted text-muted-foreground',
		low: 'bg-destructive/10 text-destructive'
	};

	let level = $derived(confidenceLevel(confidence));
</script>

<div class={cn('flex w-fit items-center gap-2 font-bold', COLORS[level], className)}>
	<span class="size-2.5 shrink-0 rounded-full bg-current" aria-hidden="true"></span>
	{m.ocr_confidence_label({ level: LABELS[level]() })}
</div>
