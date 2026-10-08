<script lang="ts">
	import { importRowNote, importRowStatus, type ImportRow } from '$lib/import';
	import ImportRows from '$lib/components/import-rows.svelte';
	import { DEFAULT_PERIOD_THRESHOLDS } from '$lib/settings';

	let rows = $state<ImportRow[]>([
		{
			keep: true,
			measuredOnDate: '2026-09-07',
			time: '07:15',
			systolic: '120',
			diastolic: '80',
			pulse: '70',
			memo: ''
		},
		{
			keep: false,
			measuredOnDate: '2026-09-07',
			time: '21:00',
			systolic: '300',
			diastolic: '80',
			pulse: '',
			memo: '',
			rowError: '上の血圧は60〜260の範囲にしてください'
		},
		{
			keep: false,
			measuredOnDate: '2026-09-08',
			time: '07:30',
			systolic: '124',
			diastolic: '82',
			pulse: '',
			memo: ''
		}
	]);

	/** テスト用: `onkeepchange` が呼ばれた時点で親が読めるチェックの状態。 */
	let seenOnKeepChange = $state('');
	/** テスト用: 「直す」で渡された行の時刻。 */
	let editedTime = $state('');
</script>

<ImportRows
	bind:rows
	periods={DEFAULT_PERIOD_THRESHOLDS}
	thisYear="2026"
	statusOf={importRowStatus}
	noteOf={importRowNote}
	onedit={(row) => (editedTime = row.time)}
	onkeepchange={() => (seenOnKeepChange = rows.map((r) => (r.keep ? '1' : '0')).join(''))}
/>
<p data-testid="seen-on-keep-change">{seenOnKeepChange}</p>
<p data-testid="edited-time">{editedTime}</p>
