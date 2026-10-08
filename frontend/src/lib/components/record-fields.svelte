<script lang="ts">
	import { halfWidthInput } from '$lib/half-width';
	import { applyMemoSuggestion, memoSuggestions } from '$lib/memo-suggestions';
	import type { RecordFormErrors } from '$lib/record-form';
	import * as m from '$lib/paraglide/messages.js';
	import * as Field from '$lib/components/ui/field';
	import { Button } from '$lib/components/ui/button';
	import { Input } from '$lib/components/ui/input';
	import { Textarea } from '$lib/components/ui/textarea';
	import FieldErrorList from '$lib/components/field-error-list.svelte';
	import RequiredMark from '$lib/components/required-mark.svelte';

	/** 血圧を入れる欄一式。記録フォームのシート・写真で記録・取り込みの行を
	 * 直すシートで共用する。見出しと送信ボタンは、シートに入れるかページに置くかを決める
	 * 呼び出し側に置く。欄の間隔も呼び出し側の `Field.FieldGroup` に委ねる (ボタンまで含めて
	 * 同じ間隔で並べるため)。 */
	let {
		measuredOnDate = $bindable(),
		time = $bindable(),
		systolic = $bindable(),
		diastolic = $bindable(),
		pulse = $bindable(),
		memo = $bindable(),
		idPrefix,
		errors = {},
		disabled = false,
		memoPhrases = []
	}: {
		/** `<input type="date">` の値 ("YYYY-MM-DD")。 */
		measuredOnDate: string;
		/** `<input type="time">` の値 ("HH:MM")。 */
		time: string;
		/** 数値の欄は入力されたままの文字列で持つ (`$lib/record-form`)。 */
		systolic: string;
		diastolic: string;
		pulse: string;
		memo: string;
		/** 欄の id の接頭辞。1つの画面に複数のフォームが出ても一意にする。 */
		idPrefix: string;
		/** 欄ごとのエラー。空なら何も出さない (入力途中の欄を赤くしないための出し分けは呼び出し側)。 */
		errors?: RecordFormErrors;
		/** 送信中など、入力を受け付けないとき。 */
		disabled?: boolean;
		/** メモの候補にする言葉。よく使う順 (`$lib/memo-suggestions`)。 */
		memoPhrases?: readonly string[];
	} = $props();

	const suggestions = $derived(memoSuggestions(memoPhrases, memo));

	const measuredAtErrorId = $derived(`${idPrefix}-measured-at-error`);
	const bpErrorId = $derived(`${idPrefix}-bp-error`);
	const pulseErrorId = $derived(`${idPrefix}-pulse-error`);
	const measuredAtDescribedBy = $derived(errors.measuredAt ? measuredAtErrorId : undefined);
</script>

<Field.Field>
	<Field.FieldLabel for="{idPrefix}-date">
		{m.record_measured_at_label()}<RequiredMark />
	</Field.FieldLabel>
	<!-- 日付と時刻は 3:2 で横に並べる。1つの `datetime-local` はモバイルで欄が画面から
	     はみ出しやすく、ピッカーの中で日付と時刻を続けて選ばされるため。 -->
	<div class="flex gap-3">
		<Input
			id="{idPrefix}-date"
			type="date"
			required
			class="w-auto min-w-0 flex-3"
			aria-invalid={errors.measuredAt !== undefined}
			aria-describedby={measuredAtDescribedBy}
			{disabled}
			bind:value={measuredOnDate}
		/>
		<Input
			id="{idPrefix}-time"
			type="time"
			required
			class="w-auto min-w-0 flex-2"
			aria-label={m.record_time_label()}
			aria-invalid={errors.measuredAt !== undefined}
			aria-describedby={measuredAtDescribedBy}
			{disabled}
			bind:value={time}
		/>
	</div>
	<FieldErrorList id={measuredAtErrorId} messages={[errors.measuredAt]} />
</Field.Field>

<!-- 入力欄とエラー表示の間は `Field` の縦並びと揃える (field.svelte の ADR)。 -->
<div class="flex flex-col gap-1.5">
	<!-- 上・下・脈拍は狭い幅でも1行に3つ並べ、値は中央寄せにする。`responsive` は
	     コンテナ幅が @md 未満だと縦積みになり、モバイルのシート幅では効かない。
	     単位付きのラベルは3列に入れると折り返して段違いになるため、短いラベルにする。 -->
	<Field.Field orientation="horizontal" class="items-end [&>*]:flex-1">
		<Field.Field>
			<Field.FieldLabel for="{idPrefix}-systolic">
				{m.record_systolic_label()}<RequiredMark />
			</Field.FieldLabel>
			<Input
				id="{idPrefix}-systolic"
				type="text"
				inputmode="numeric"
				{@attach halfWidthInput}
				required
				class="text-center text-lg font-bold md:text-lg"
				aria-invalid={errors.systolic !== undefined}
				aria-describedby={errors.systolic ? bpErrorId : undefined}
				{disabled}
				bind:value={systolic}
			/>
		</Field.Field>
		<Field.Field>
			<Field.FieldLabel for="{idPrefix}-diastolic">
				{m.record_diastolic_label()}<RequiredMark />
			</Field.FieldLabel>
			<Input
				id="{idPrefix}-diastolic"
				type="text"
				inputmode="numeric"
				{@attach halfWidthInput}
				required
				class="text-center text-lg font-bold md:text-lg"
				aria-invalid={errors.diastolic !== undefined}
				aria-describedby={errors.diastolic ? bpErrorId : undefined}
				{disabled}
				bind:value={diastolic}
			/>
		</Field.Field>
		<Field.Field>
			<Field.FieldLabel for="{idPrefix}-pulse">{m.record_pulse_label()}</Field.FieldLabel>
			<Input
				id="{idPrefix}-pulse"
				type="text"
				inputmode="numeric"
				{@attach halfWidthInput}
				class="text-center text-lg font-bold md:text-lg"
				aria-invalid={errors.pulse !== undefined}
				aria-describedby={errors.pulse ? pulseErrorId : undefined}
				{disabled}
				bind:value={pulse}
			/>
		</Field.Field>
	</Field.Field>
	<FieldErrorList id={bpErrorId} messages={[errors.systolic, errors.diastolic]} />
	<FieldErrorList id={pulseErrorId} messages={[errors.pulse]} />
</div>

<Field.Field>
	<Field.FieldLabel for="{idPrefix}-memo">{m.record_memo_label()}</Field.FieldLabel>
	<!-- メモは空のことが多いので、入力欄と同じ1行分の高さから始め、書いた分だけ伸ばす
	     (field-sizing に対応しないブラウザでは伸びずにスクロールする)。 -->
	<Textarea id="{idPrefix}-memo" class="min-h-11" {disabled} bind:value={memo} />
	<!-- 過去のメモの言葉をチップで出し、タップで入れる。 -->
	{#if suggestions.length > 0}
		<div class="flex flex-wrap gap-2" role="group" aria-label={m.record_memo_suggestions_label()}>
			{#each suggestions as phrase (phrase)}
				<Button
					type="button"
					variant="outline"
					class="h-10 max-w-full rounded-full px-3.5 font-medium"
					{disabled}
					onclick={() => (memo = applyMemoSuggestion(memoPhrases, memo, phrase))}
				>
					<!-- 区切らずに書いた長い文も候補になるので、画面の幅を超える分は「…」で省く (入れるのは全文)。 -->
					<span class="truncate">{phrase}</span>
				</Button>
			{/each}
		</div>
	{/if}
</Field.Field>
