<script lang="ts">
	import { onMount, tick, type Snippet } from 'svelte';
	import { userTimeZone } from '$lib/auth';
	import { fetchMemoPhrases } from '$lib/memo-suggestions';
	import type { CreateRecordRequest } from '$lib/records';
	import { validateRecordForm, type RecordFormErrors } from '$lib/record-form';
	import * as Field from '$lib/components/ui/field';
	import RecordFields from '$lib/components/record-fields.svelte';

	/** 血圧を入れるフォーム。欄・検証・送信までを受け持ち、シートに入れるか
	 * ページに置くかは外側で決める (記録フォーム・取り込みの行を直すシートは
	 * `record-form-sheet.svelte`、写真で記録 (1件) はページに直接置く)。
	 *
	 * 入力値は外側が持ち、`bind:` で渡す。シートは閉じる前の確認に、写真で記録は読み取り結果の
	 * 反映に、入力値を使うため。 */
	let {
		id,
		idPrefix = 'record',
		notice,
		measuredOnDate = $bindable(),
		time = $bindable(),
		systolic = $bindable(),
		diastolic = $bindable(),
		pulse = $bindable(),
		memo = $bindable(),
		submitting = $bindable(false),
		errorsOnOpen = false,
		onsubmit,
		onsubmitted,
		actions
	}: {
		/** フォームの id。フォームの外に置く送信ボタンから `form` 属性で指すのに使う。 */
		id?: string;
		/** 欄の id の接頭辞。 */
		idPrefix?: string;
		/** 欄の上に出す注意書き。無ければ出さない。 */
		notice?: string;
		measuredOnDate: string;
		time: string;
		systolic: string;
		diastolic: string;
		pulse: string;
		memo: string;
		/** 開いた時点から入力エラーを出すか。取り込みの記録を直すときに使う (docs/import-export.md)。 */
		errorsOnOpen?: boolean;
		/** 送信中か。送信している間は欄を無効にする。 */
		submitting?: boolean;
		/** 検証を通った値で送信する。成功なら `true` を返す。`false` ならエラーの表示は
		 * 呼び出し側が受け持つ。 */
		onsubmit: (record: CreateRecordRequest) => boolean | Promise<boolean>;
		/** 送信が成功したとき。 */
		onsubmitted?: () => void;
		/** 欄の後に並べるボタン。フォームの中に置くので、`type="submit"` のボタンで送信できる。 */
		actions?: Snippet;
	} = $props();

	let form = $state<HTMLFormElement | null>(null);

	/** メモの候補にする言葉。フォームを作るたびに取り直す (登録したメモがすぐ候補に入るように)。
	 * 取れなければ候補を出さないだけで、入力はそのままできる。 */
	let memoPhrases = $state<string[]>([]);
	onMount(() => {
		let active = true;
		Promise.resolve()
			.then(() => fetchMemoPhrases(userTimeZone()))
			.then((phrases) => {
				if (active) memoPhrases = phrases;
			})
			.catch(() => {});
		return () => {
			active = false;
		};
	});

	/** 入力エラーは一度送信しようとするまで出さない (入力途中の欄を赤くしないため)。`errorsOnOpen` なら
	 * 開いた時点から出す。出した後は入力のたびに検証し直し、直した欄から消す。 */
	let submitted = $state(false);
	const showErrors = $derived(errorsOnOpen || submitted);
	const validation = $derived(
		validateRecordForm({ measuredOnDate, time, systolic, diastolic, pulse })
	);
	const errors: RecordFormErrors = $derived(showErrors && !validation.ok ? validation.errors : {});

	async function handleSubmit(e: SubmitEvent) {
		e.preventDefault();
		if (submitting) return;

		submitted = true;
		if (!validation.ok) {
			// シートの高さを超える場合、エラーの欄がスクロールの外に隠れていることがあるため。
			await tick();
			const first = form?.querySelector<HTMLElement>('[aria-invalid="true"]');
			// 日付・時刻の欄はフォーカスするとモバイルでピッカーが開き、直下のエラー文言を覆う。
			// 見える位置まで送るだけにする。
			if (first instanceof HTMLInputElement && (first.type === 'date' || first.type === 'time')) {
				first.scrollIntoView({ block: 'nearest' });
			} else {
				first?.focus();
			}
			return;
		}

		submitting = true;
		const ok = await onsubmit({
			localMeasuredAt: validation.measuredAtLocal,
			systolic: validation.systolic,
			diastolic: validation.diastolic,
			pulse: validation.pulse,
			memo
		});
		submitting = false;
		if (ok) onsubmitted?.();
	}
</script>

{#if notice}
	<p class="mb-4 text-sm text-muted-foreground">{notice}</p>
{/if}

<!-- 未入力はブラウザの検証 (欄の `required`) に任せ、値域と欄をまたぐ検証 (収縮期 > 拡張期) だけを
     欄の直下に出す (AGENTS.md「コードの規約」)。 -->
<form bind:this={form} {id} onsubmit={handleSubmit}>
	<Field.FieldGroup>
		<RecordFields
			{idPrefix}
			{errors}
			disabled={submitting}
			{memoPhrases}
			bind:measuredOnDate
			bind:time
			bind:systolic
			bind:diastolic
			bind:pulse
			bind:memo
		/>
		{@render actions?.()}
	</Field.FieldGroup>
</form>
