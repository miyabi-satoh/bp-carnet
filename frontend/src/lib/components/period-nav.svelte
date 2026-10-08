<script lang="ts">
	import { untrack } from 'svelte';
	import * as m from '$lib/paraglide/messages.js';
	import { userTimeZone } from '$lib/auth';
	import {
		detectPeriodMode,
		formatDateOnly,
		formatOpenPeriodLabel,
		formatDateRangeLabel,
		localToday,
		parseLocalDate,
		periodFor,
		shiftAnchor,
		splitPeriodLabel,
		type Period
	} from '$lib/period';
	import ButtonRow from '$lib/components/button-row.svelte';
	import FieldErrorList from '$lib/components/field-error-list.svelte';
	import * as Field from '$lib/components/ui/field';
	import { Button } from '$lib/components/ui/button';
	import { Input } from '$lib/components/ui/input';
	import LoadingButton from '$lib/components/loading-button.svelte';
	import RoundButton from '$lib/components/round-button.svelte';
	import ChevronDownIcon from '@lucide/svelte/icons/chevron-down';
	import ChevronLeftIcon from '@lucide/svelte/icons/chevron-left';
	import ChevronRightIcon from '@lucide/svelte/icons/chevron-right';

	/** 週/月ナビゲーションと任意期間の指定。メイン画面と印刷レポートで共有する。
	 *
	 * 期間を変える操作をすると `onchange` で新しい期間を知らせる。記録の取得やURLへの反映は
	 * 呼び出し側で行う。 */
	let {
		from,
		to,
		loading = false,
		onCard = false,
		timeZone,
		defaultAnchor,
		onchange
	}: {
		/** 表示中の期間 (YYYY-MM-DD、空文字は指定なし)。 */
		from: string;
		to: string;
		/** 取得中は期間を変える操作を受け付けない。 */
		loading?: boolean;
		/** カードの中に置くとき (印刷レポート)。選択中のタブと前後ボタンの
		 * 地を、周りのカードと見分けられるページ地の色にする。 */
		onCard?: boolean;
		/** 「今日」を求めるタイムゾーン。省略時はログイン中ユーザーの個人設定 (管理者の閲覧では
		 * 対象ユーザーのものを渡す)。 */
		timeZone?: string;
		/** 初期表示の基準日 (メイン画面では最新の記録の日)。「クリア」はこの日を含む週に戻す。表示中の
		 * 週/月がこの日を含むときは、これを基準日にする (週の始まりが前の月でも、月タブで
		 * この日の月を出すため)。省略時は今日。 */
		defaultAnchor?: Date;
		onchange: (from: string, to: string) => void;
	} = $props();

	/** 選択中のタブと前後ボタンの地の色。 */
	let raisedBg = $derived(onCard ? 'bg-background' : 'bg-card');

	const periodTabs = [
		{ mode: 'week', label: m.period_nav_week_button() },
		{ mode: 'month', label: m.period_nav_month_button() }
	] as const;

	/** 個人設定のタイムゾーンでの今日。 */
	function today(): Date {
		return localToday(timeZone ?? userTimeZone());
	}

	/** 期間から決まる、週/月のモード・基準日・入力欄の値。 */
	function stateFor(from: string, to: string) {
		const detected = detectPeriodMode(from, to, today());
		const defaultDate = defaultAnchor ? formatDateOnly(defaultAnchor) : null;
		const containsDefault = defaultDate !== null && from <= defaultDate && defaultDate <= to;
		const anchor = detected && containsDefault ? defaultAnchor! : (detected?.anchor ?? today());
		return { mode: detected?.mode ?? null, anchor, from, to };
	}

	const initial = untrack(() => stateFor(from, to));

	/** `null` は「from/to ピッカーによる任意期間の絞り込み中」を表す。 */
	let periodMode = $state<'week' | 'month' | null>(initial.mode);
	/** 週/月ナビゲーションの基準日。 */
	let periodAnchor = $state<Date>(initial.anchor);

	/** 任意期間の入力欄 (`<input type="date">` 用、YYYY-MM-DD)。「絞り込む」を押すまでは未適用。 */
	let filterFromDate = $state(initial.from);
	let filterToDate = $state(initial.to);

	/** 最後に `onchange` で知らせた期間 (最初は渡された期間)。表示には使わないため `$state` にしない。 */
	let notified = { from: initial.from, to: initial.to };

	// 知らせた期間とは違う期間を外から渡されたら (例: 同じページ内の遷移でURLクエリが変わった)、
	// モード・基準日・入力欄をその期間に合わせ直す。知らせた期間がそのまま返ってきただけなら
	// 合わせ直さない。期間から決め直すと、操作で決めた基準日 (週⇔月の切り替えで使う) が変わるため。
	$effect(() => {
		if (from === notified.from && to === notified.to) return;
		const next = stateFor(from, to);
		periodMode = next.mode;
		periodAnchor = next.anchor;
		filterFromDate = next.from;
		filterToDate = next.to;
		notified = { from, to };
	});

	/** from/to ピッカーの折りたたみ開閉状態。 */
	let filterPanelOpen = $state(false);

	/** `periodMode` が `null` でなければ、現在表示中の週/月の期間。 */
	let currentPeriod = $derived(
		periodMode !== null ? periodFor(periodMode, periodAnchor, today()) : null
	);

	/** 見出しラベル。週/月タブ選択中はその期間、任意期間の絞り込み中は指定した範囲を表示する。 */
	let periodLabel = $derived.by(() => {
		if (currentPeriod) return currentPeriod.label;
		return (
			formatOpenPeriodLabel(from, to) ??
			formatDateRangeLabel(parseLocalDate(from), parseLocalDate(to))
		);
	});

	function notifyChange() {
		notified = { from: filterFromDate, to: filterToDate };
		onchange(filterFromDate, filterToDate);
	}

	/** 計算した期間を入力欄に反映する。 */
	function applyPeriod(period: Period) {
		filterFromDate = formatDateOnly(period.from);
		filterToDate = formatDateOnly(period.to);
	}

	/** 「絞り込む」ボタン押下時。手動指定した範囲は週/月の境界と一致しているとは限らないため、
	 * 週/月ナビゲーションは解除する。 */
	function applyManualFilter() {
		filterSubmitted = true;
		if (filterRangeError) return;
		filterSubmitted = false;
		periodMode = null;
		notifyChange();
	}

	/** 開始日が終了日より後なら、送る前に止めて欄の下で知らせる (どこを直せばよいかを示すため)。
	 * 「絞り込む」を押すまでは出さない (入力の途中で出さないため)。 */
	let filterSubmitted = $state(false);
	let filterRangeError = $derived(
		filterSubmitted && filterFromDate !== '' && filterToDate !== '' && filterFromDate > filterToDate
			? m.period_nav_filter_range_error()
			: undefined
	);

	/** 「クリア」ボタン押下時、任意期間の絞り込みを解除して初期表示の週 (`defaultAnchor` を含む週、
	 * 無ければ今週) に戻す (全件表示には戻さない。期間ナビが常時「週」または「月」のどちらかを
	 * 表示する状態を保つため)。 */
	function clearFilter() {
		filterPanelOpen = false;
		periodAnchor = defaultAnchor ?? today();
		switchPeriodMode('week');
	}

	/** 週/月タブ押下時、基準日はそのままに (今日へリセットせず) 表示中の期間をそのモードで
	 * 引き直す。例えば9月第2週を表示中に月タブへ切り替えたら9月を表示する。 */
	function switchPeriodMode(mode: 'week' | 'month') {
		periodMode = mode;
		applyPeriod(periodFor(mode, periodAnchor, today()));
		notifyChange();
	}

	/** 「前へ」/「次へ」ボタン押下時、表示中の週/月を ±1 期間ずらす。 */
	function shiftPeriod(direction: -1 | 1) {
		if (periodMode === null) return;
		periodAnchor = shiftAnchor(periodMode, periodAnchor, direction);
		applyPeriod(periodFor(periodMode, periodAnchor, today()));
		notifyChange();
	}

	/** 「今週へ」/「今月へ」ボタン押下時、基準日を今日に戻す。 */
	function jumpToToday() {
		periodAnchor = today();
		if (periodMode !== null) {
			applyPeriod(periodFor(periodMode, periodAnchor, today()));
		}
		notifyChange();
	}
</script>

<div class="flex flex-col gap-3">
	<!-- 前後ボタンの見た目はデザイン (浮かせる)。置き場所は、デザインの期間の左右から週/月の切り替えの左右へ移した。
	     期間に1行を全部使わせ、年をまたぐ指定の期間でも狭い画面で折り返さないようにするため。
	     週/月の切り替えは、選ばれていない側が押せるように見えなかったため、ひと続きの帯にして
	     デザインから変えた。
	     ADR: shadcn の Button は基底で枠線を透明にし、ghost のホバー色も持っていて、この見た目と
	     ぶつかるため素の button で組む。 -->
	{#snippet shiftButton(direction: -1 | 1)}
		<RoundButton
			class="raised {raisedBg}"
			aria-label={direction < 0 ? m.period_nav_prev_button() : m.period_nav_next_button()}
			disabled={!currentPeriod || loading}
			onclick={() => shiftPeriod(direction)}
		>
			{#if direction < 0}
				<ChevronLeftIcon size={20} strokeWidth={2.2} aria-hidden="true" />
			{:else}
				<ChevronRightIcon size={20} strokeWidth={2.2} aria-hidden="true" />
			{/if}
		</RoundButton>
	{/snippet}
	<div class="flex items-center gap-2">
		{@render shiftButton(-1)}
		<div class="grid flex-1 grid-cols-2 gap-1 rounded-md border border-border p-1">
			{#each periodTabs as tab (tab.mode)}
				{@const selected = periodMode === tab.mode}
				<button
					type="button"
					class={[
						'h-11 rounded-sm text-sm transition-colors outline-none focus-visible:ring-3 focus-visible:ring-ring/50 disabled:pointer-events-none disabled:opacity-50',
						selected
							? ['border border-primary font-bold text-accent-foreground', raisedBg]
							: 'font-medium text-foreground hover:text-accent-foreground'
					]}
					aria-pressed={selected}
					disabled={loading}
					onclick={() => switchPeriodMode(tab.mode)}
				>
					{tab.label}
				</button>
			{/each}
		</div>
		{@render shiftButton(1)}
	</div>
	<!-- ラベルのタップで今週/今月へジャンプする (デザインに専用ボタンが無いため集約)。
	     読み上げの名前は、見えている期間の後に行き先を添える (見えている文字を名前の頭に含める。WCAG 2.5.3)。 -->
	<button
		type="button"
		class="min-h-11 text-sm font-bold disabled:pointer-events-none disabled:opacity-100"
		disabled={!currentPeriod || loading}
		aria-label={currentPeriod
			? periodMode === 'week'
				? m.period_nav_this_week_button({ period: periodLabel })
				: m.period_nav_this_month_button({ period: periodLabel })
			: undefined}
		onclick={jumpToToday}
	>
		{#each splitPeriodLabel(periodLabel) as part, i (i)}{#if i > 0}<wbr />{/if}<span
				class="whitespace-nowrap">{part}</span
			>{/each}
	</button>
	<details bind:open={filterPanelOpen} class="group text-sm">
		<!-- 開閉の矢印を添えて押せるものだと分かるようにし、押せる範囲は文字と矢印の幅だけにする
		     (`flex` で行いっぱいにすると、文字の無い右側を押しても開いてしまうため)。 -->
		<summary class="inline-flex min-h-11 items-center gap-1 text-muted-foreground select-none"
			>{m.period_nav_filter_toggle_label()}<ChevronDownIcon
				size={16}
				strokeWidth={2.2}
				class="transition-transform group-open:rotate-180"
				aria-hidden="true"
			/></summary
		>
		<Field.FieldGroup class="mt-3">
			<Field.Field orientation="responsive" class="flex-wrap items-end @md/field-group:items-end">
				<!-- 開始・終了は年月日だけなので、どの幅でも2つ横に並べる。広い画面ではボタンと同じ行に置く。
				     ADR: 箱を溶かして (`contents`) 並べると、開始・終了の Field がそれぞれ行の幅いっぱいを取り、
				     折り返して縦に積まれる (iPad・PC で起きていた)。 -->
				<div class="grid grid-cols-2 gap-3 @md/field-group:flex-1">
					<Field.Field class="min-w-0">
						<Field.FieldLabel for="filter-from">{m.period_nav_filter_from_label()}</Field.FieldLabel
						>
						<!-- 日付欄は素の flex の箱に入れ、`w-auto` にする。Field の縦並びは子に `w-full` を強制し、iOS の
						     日付欄はそれだと枠線と余白の分だけ右にはみ出すため (記録フォームの日付欄と同じ対策)。 -->
						<div class="flex">
							<Input
								id="filter-from"
								type="date"
								class="w-auto min-w-0 flex-1"
								disabled={loading}
								bind:value={
									() => filterFromDate,
									(v) => {
										filterFromDate = v;
										periodMode = null;
									}
								}
							/>
						</div>
					</Field.Field>
					<Field.Field class="min-w-0">
						<Field.FieldLabel for="filter-to">{m.period_nav_filter_to_label()}</Field.FieldLabel>
						<div class="flex">
							<Input
								id="filter-to"
								type="date"
								class="w-auto min-w-0 flex-1"
								disabled={loading}
								aria-invalid={filterRangeError !== undefined}
								aria-describedby={filterRangeError ? 'filter-range-error' : undefined}
								bind:value={
									() => filterToDate,
									(v) => {
										filterToDate = v;
										periodMode = null;
									}
								}
							/>
						</div>
					</Field.Field>
				</div>
				<ButtonRow>
					<Button type="button" variant="outline" disabled={loading} onclick={clearFilter}>
						{m.period_nav_filter_clear_button()}
					</Button>
					<LoadingButton type="button" {loading} onclick={applyManualFilter}>
						{m.period_nav_filter_apply_button()}
					</LoadingButton>
				</ButtonRow>
			</Field.Field>
			<FieldErrorList id="filter-range-error" messages={[filterRangeError]} />
		</Field.FieldGroup>
	</details>
</div>
