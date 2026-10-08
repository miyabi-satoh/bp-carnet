<script lang="ts">
	import CircleAlertIcon from '@lucide/svelte/icons/circle-alert';
	import * as Field from '$lib/components/ui/field';

	/** 欄の直下に出す入力エラー。横に並ぶ欄のエラーは並びの下にまとめて出し、同じ文言は1つにする
	 * (上下の血圧の大小関係のように、2つの欄に同じ文言が入るエラーがあるため)。 */
	let {
		id,
		messages
	}: {
		/** 欄の `aria-describedby` から参照する id。 */
		id: string;
		/** 欄ごとのエラー。`undefined` はエラー無し。 */
		messages: (string | undefined)[];
	} = $props();

	let unique = $derived([...new Set(messages.filter((message) => message !== undefined))]);
</script>

{#if unique.length > 0}
	<Field.FieldError {id} class="flex flex-col gap-1 text-xs">
		{#each unique as message (message)}
			<p class="flex items-center gap-1.5">
				<CircleAlertIcon size={14} class="shrink-0" aria-hidden="true" />
				{message}
			</p>
		{/each}
	</Field.FieldError>
{/if}
