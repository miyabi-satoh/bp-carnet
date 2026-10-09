<script lang="ts" generics="T extends object">
	import type { ApiResult } from '$lib/api/errors';
	import DialogAlertTitleRow from '$lib/components/dialog-alert-title-row.svelte';
	import DialogFormFooter from '$lib/components/dialog-form-footer.svelte';
	import DialogResult from '$lib/components/dialog-result.svelte';
	import LoadingButton from '$lib/components/loading-button.svelte';
	import NoticeDialog from '$lib/components/notice-dialog.svelte';
	import RequiredMark from '$lib/components/required-mark.svelte';
	import * as Dialog from '$lib/components/ui/dialog';
	import * as Field from '$lib/components/ui/field';
	import { Input } from '$lib/components/ui/input';
	import { busyCloseGuard, DESTRUCTIVE_ACTION_CLASS } from '$lib/dialog';

	/** 決めた語を打ち込ませてから削除を予約する確認モーダル (本人の退会・管理者のユーザー削除)。
	 * 違うのは文言と呼ぶ API だけなので、入れ物 (`delete-account-dialog.svelte` など) が渡す。
	 * 開くのは `bind:this` + `show()`。 */
	type Props = {
		title: string;
		description: string;
		confirmLabel: string;
		/** 確認欄に打ち込ませる語。前後の空白は無視して比べる。 */
		confirmWord: string;
		inputId: string;
		submitLabel: string;
		errorTitle: string;
		scheduledTitle: string;
		scheduledDescription: (result: T) => string;
		schedule: () => Promise<ApiResult<T>>;
		/** 予約できて結果に差し替えた後に呼ぶ。 */
		onscheduled?: (result: T) => void | Promise<void>;
	};

	let {
		title,
		description,
		confirmLabel,
		confirmWord,
		inputId,
		submitLabel,
		errorTitle,
		scheduledTitle,
		scheduledDescription,
		schedule,
		onscheduled
	}: Props = $props();

	let open = $state(false);
	let confirmation = $state('');
	let loading = $state(false);
	const closeGuard = busyCloseGuard(() => loading);
	/** 予約できたら同じモーダルの中身を結果に差し替える (削除は一度で完了する操作ではなく、
	 * 「いつ消えるか」「どうすれば戻せるか」を読んでもらう必要があるため)。 */
	let scheduled = $state<T | null>(null);
	let errorDialog = $state<ReturnType<typeof NoticeDialog> | null>(null);

	let canSubmit = $derived(confirmWord !== '' && confirmation.trim() === confirmWord);

	export function show() {
		confirmation = '';
		scheduled = null;
		open = true;
	}

	async function submit(event: SubmitEvent) {
		event.preventDefault();
		if (loading || !canSubmit) return;

		loading = true;
		const result = await schedule();
		loading = false;
		if (!result.ok) {
			errorDialog?.show(errorTitle, result.message);
			return;
		}
		scheduled = result;
		await onscheduled?.(result);
	}
</script>

<Dialog.Root bind:open>
	<!-- 戻せない操作の確認: × を出さず、警告アイコンと赤い実行ボタン。 -->
	<Dialog.Content {...closeGuard}>
		{#if scheduled !== null}
			<DialogResult title={scheduledTitle} description={scheduledDescription(scheduled)} />
		{:else}
			<Dialog.Header>
				<DialogAlertTitleRow>
					<Dialog.Title>{title}</Dialog.Title>
				</DialogAlertTitleRow>
				<Dialog.Description>{description}</Dialog.Description>
			</Dialog.Header>
			<form onsubmit={submit}>
				<Field.Field>
					<Field.FieldLabel for={inputId}>
						{confirmLabel}<RequiredMark />
					</Field.FieldLabel>
					<Input
						id={inputId}
						class="border-destructive"
						required
						autocomplete="off"
						autocapitalize="off"
						bind:value={confirmation}
					/>
				</Field.Field>
				<DialogFormFooter {loading} oncancel={() => (open = false)}>
					<LoadingButton
						type="submit"
						variant="destructive"
						class={DESTRUCTIVE_ACTION_CLASS}
						disabled={!canSubmit}
						{loading}
					>
						{submitLabel}
					</LoadingButton>
				</DialogFormFooter>
			</form>
		{/if}
	</Dialog.Content>
</Dialog.Root>

<NoticeDialog bind:this={errorDialog} />
