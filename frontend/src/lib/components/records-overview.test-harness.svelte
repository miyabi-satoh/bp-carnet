<script lang="ts">
	import RecordsOverview from '$lib/components/records-overview.svelte';
	import type { ChartSeriesKey } from '$lib/bp-chart';
	import type { BpRecord } from '$lib/records';
	import { RECORD_LIST_PAGE_SIZE } from '$lib/records-view.svelte';

	let { count }: { count: number } = $props();

	/** テスト用: 1日1件、新しい順に `count` 件の記録。メモに通し番号を入れて見分ける。 */
	const records: BpRecord[] = $derived(
		Array.from({ length: count }, (_, i) => ({
			id: i + 1,
			localMeasuredAt: `2026-09-${String(28 - (i % 28)).padStart(2, '0')}T07:00`,
			systolic: 120,
			diastolic: 80,
			pulse: null,
			memo: `記録${i + 1}`,
			version: 1
		}))
	);
	let visibleKeys = $state<ChartSeriesKey[]>([]);
	let listLimit = $state(RECORD_LIST_PAGE_SIZE);
</script>

<RecordsOverview
	{records}
	summary={{ days: [] }}
	loading={false}
	errorMessage=""
	onretry={async () => {}}
	bind:visibleKeys
	bind:listLimit
	timeZone="Asia/Tokyo"
/>
