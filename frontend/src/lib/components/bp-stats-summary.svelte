<script lang="ts">
	import type { components } from '$lib/api/schema';
	import * as m from '$lib/paraglide/messages.js';
	import { cn } from '$lib/utils';

	type BpAverage = components['schemas']['BpAverageResponse'];

	let {
		morning,
		evening,
		unit = 'caption',
		class: className = ''
	}: {
		morning: BpAverage | null | undefined;
		evening: BpAverage | null | undefined;
		/** 単位の出し方。メイン画面はカード下の説明、印刷レポート
		 * は値の横に「mmHg」だけを添える。 */
		unit?: 'caption' | 'inline';
		/** 並べ方を画面ごとに変えるためのクラス (例: 紙面では縦積み)。 */
		class?: string;
	} = $props();

	/** 見出しの色は朝/夜の線と同じ色味にして、表と線を目で結びつけられるようにする。画面では
	 * 文字として読める濃さの色、紙面では線そのものの色
	 * を使う。 */
	const cards = $derived([
		{
			label: m.bp_stats_morning_average_label(),
			labelClass: 'text-teal-foreground print:text-chart-1',
			average: morning
		},
		{
			label: m.bp_stats_evening_average_label(),
			labelClass: 'text-coral-foreground print:text-chart-2',
			average: evening
		}
	]);

	/** 対象の記録が無い区分は API が null を返す。空欄ではなく「-」で埋めて、値が無かったことを
	 * はっきり示す。 */
	function format(value: number | undefined): string {
		return value === undefined ? m.common_no_data() : String(Math.round(value));
	}
</script>

<div class={cn('grid grid-cols-2 gap-3', className)}>
	{#each cards as card (card.label)}
		<!-- 紙面は影を出さず枠線で囲む。 -->
		<div class="rounded-lg raised bg-card px-4 py-3.5 print:border print:p-3 print:shadow-none">
			<div class={['mb-1 text-sm font-bold', card.labelClass]}>{card.label}</div>
			<div class="text-2xl font-bold">
				{format(card.average?.systolic)}<span
					class="px-1 text-base font-medium text-muted-foreground">/</span
				>{format(card.average?.diastolic)}{#if unit === 'inline'}<span
						class="ml-1 text-sm font-medium text-muted-foreground">mmHg</span
					>{/if}
			</div>
			{#if unit === 'caption'}
				<div class="mt-0.5 text-xs text-muted-foreground">{m.bp_stats_unit_caption()}</div>
			{/if}
		</div>
	{/each}
</div>
