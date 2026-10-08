<script lang="ts">
	import { buttonVariants } from '$lib/components/ui/button';
	import * as Popover from '$lib/components/ui/popover';
	import * as m from '$lib/paraglide/messages.js';
	import {
		DAY_MINUTES_MAX,
		TIME_PICKER_STEP_MIN,
		minutesToTimeText,
		withHour,
		withMinute
	} from '$lib/period';

	/** 時刻の欄。タップすると「時」「分」を選ぶポップオーバーを開き、
	 * どちらかを選ぶたびに `value` を変える。 */
	let {
		value = $bindable(),
		label,
		invalid = false,
		describedby
	}: {
		/** 0:00 からの経過分数 (0〜1440)。 */
		value: number;
		/** 欄の名前 (例: 朝の開始時刻)。 */
		label: string;
		invalid?: boolean;
		/** エラー文言の要素の id。 */
		describedby?: string;
	} = $props();

	const LAST_HOUR = DAY_MINUTES_MAX / 60;
	const HOURS = Array.from({ length: LAST_HOUR + 1 }, (_, hour) => hour);
	const MINUTES = Array.from(
		{ length: 60 / TIME_PICKER_STEP_MIN },
		(_, i) => i * TIME_PICKER_STEP_MIN
	);

	const id = $props.id();
	let content = $state<HTMLElement | null>(null);

	let hour = $derived(Math.floor(value / 60));
	let minute = $derived(value % 60);
</script>

{#snippet option(text: string, selected: boolean, disabled: boolean, onclick: () => void)}
	<button
		type="button"
		class={[
			'h-11 rounded-sm text-base tabular-nums transition-colors outline-none focus-visible:ring-3 focus-visible:ring-ring/50 disabled:pointer-events-none disabled:opacity-50',
			selected ? 'bg-primary font-bold text-primary-foreground' : 'hover:bg-muted'
		]}
		aria-pressed={selected}
		{disabled}
		{onclick}
	>
		{text}
	</button>
{/snippet}

<Popover.Root>
	<!-- 見た目は入力欄 (`ui/input`) に揃える。 -->
	<Popover.Trigger
		class="h-11 w-16 rounded-sm border border-input bg-field text-base tabular-nums transition-colors outline-none focus-visible:border-ring focus-visible:ring-3 focus-visible:ring-ring/50 aria-invalid:border-destructive aria-invalid:ring-3 aria-invalid:ring-destructive/20 dark:aria-invalid:border-destructive/50 dark:aria-invalid:ring-destructive/40"
		aria-invalid={invalid}
		aria-describedby={describedby}
	>
		<span class="sr-only">{label}</span>
		{minutesToTimeText(value)}
	</Popover.Trigger>
	<Popover.Content
		bind:ref={content}
		class="gap-3 p-4"
		aria-labelledby="{id}-title"
		onOpenAutoFocus={(event) => {
			// 先頭の「0」ではなく、選択中の「時」から操作を始められるようにする。
			event.preventDefault();
			content?.querySelector<HTMLElement>('[aria-pressed="true"]')?.focus();
		}}
	>
		<p id="{id}-title" class="font-bold">{label}</p>
		<div role="group" aria-labelledby="{id}-hour" class="flex flex-col gap-1.5">
			<span id="{id}-hour" class="text-xs text-muted-foreground">{m.time_picker_hour_label()}</span>
			<div class="grid grid-cols-5 gap-1.5">
				{#each HOURS as h (h)}
					{@render option(String(h), h === hour, false, () => (value = withHour(value, h)))}
				{/each}
			</div>
		</div>
		<div role="group" aria-labelledby="{id}-minute" class="flex flex-col gap-1.5">
			<span id="{id}-minute" class="text-xs text-muted-foreground"
				>{m.time_picker_minute_label()}</span
			>
			<div class="grid grid-cols-2 gap-1.5">
				{#each MINUTES as mm (mm)}
					<!-- 24時は 24:00 だけを選べる。 -->
					{@render option(
						String(mm).padStart(2, '0'),
						mm === minute,
						hour === LAST_HOUR && mm !== 0,
						() => (value = withMinute(value, mm))
					)}
				{/each}
			</div>
		</div>
		<Popover.Close class={buttonVariants({ variant: 'outline' })}>
			{m.common_close_button()}
		</Popover.Close>
	</Popover.Content>
</Popover.Root>
