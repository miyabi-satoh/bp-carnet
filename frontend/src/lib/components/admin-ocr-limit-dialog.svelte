<script lang="ts">
	import { setUserOcrLimit, type AdminUser } from '$lib/admin';
	import { userDisplayName } from '$lib/auth';
	import DialogFormHeader from '$lib/components/dialog-form-header.svelte';
	import DialogFormFooter from '$lib/components/dialog-form-footer.svelte';
	import LoadingButton from '$lib/components/loading-button.svelte';
	import NoticeDialog from '$lib/components/notice-dialog.svelte';
	import * as Dialog from '$lib/components/ui/dialog';
	import * as Field from '$lib/components/ui/field';
	import { busyCloseGuard, focusFirstFieldOnOpen } from '$lib/dialog';
	import * as m from '$lib/paraglide/messages.js';
	import { inRange, parseStrictInt } from '$lib/utils';
	import { FIELD_SELECT_CLASS } from '$lib/styles';
	import FieldErrorList from '$lib/components/field-error-list.svelte';
	import { Input } from '$lib/components/ui/input';
	import RequiredMark from '$lib/components/required-mark.svelte';
	import { halfWidthInput } from '$lib/half-width';

	/** バックエンドの `src/validation.rs` と同じ値域・無制限の値。送信前に弾き、422 で往復するのを避ける。 */
	const BUDGET_RANGE = [0, 100000] as const;
	const UNLIMITED = -1;

	/** OCR上限の変更モーダル。ユーザー一覧のカードから `bind:this` + `show()` で開く
	 * (`delete-account-dialog.svelte` と同じパターン)。 */
	let { onsaved }: { onsaved: () => void } = $props();
	/** 上限の指定方法。`custom` のときだけ金額の入力欄を出す。 */
	type Mode = 'default' | 'unlimited' | 'custom';

	let open = $state(false);
	let user = $state<AdminUser | null>(null);
	let defaultBudget = $state(0);
	let budgetMode = $state<Mode>('default');
	/** 累計金額 (円) の入力値。記録フォームと同じく入力されたままの文字列で持つ (数字以外の入力を
	 * 「未入力」と区別してエラーにするため)。 */
	let budget = $state('');
	let loading = $state(false);
	const closeGuard = busyCloseGuard(() => loading);
	let content = $state<HTMLElement | null>(null);
	const focusOnOpen = focusFirstFieldOnOpen(() => content);
	/** 入力中の欄にエラーを出し続けないよう、一度送信するまでは欄の下に何も出さない。 */
	let submitted = $state(false);
	let errorDialog = $state<ReturnType<typeof NoticeDialog> | null>(null);

	/** 入力された金額。値域外・数字以外なら `null`。 */
	function parseBudget(value: string): number | null {
		const parsed = parseStrictInt(value);
		return parsed === null || !inRange(parsed, BUDGET_RANGE) ? null : parsed;
	}
	let budgetError = $derived(
		budgetMode === 'custom' && submitted && parseBudget(budget) === null
			? m.admin_users_ocr_budget_error({ min: BUDGET_RANGE[0], max: BUDGET_RANGE[1] })
			: undefined
	);

	/** 既定値そのものが無制限に設定されていることもあるため、選択肢の文言を出し分ける。 */
	let defaultBudgetModeLabel = $derived(
		defaultBudget < 0
			? m.admin_users_ocr_budget_mode_default_unlimited()
			: m.admin_users_ocr_budget_mode_default({ yen: defaultBudget })
	);
	function modeFor(value: number | null | undefined): Mode {
		return value == null ? 'default' : value < 0 ? 'unlimited' : 'custom';
	}
	function inputFor(value: number | null | undefined): string {
		return value != null && value >= 0 ? String(value) : '';
	}
	function valueFor(mode: Mode, parsed: number | null): number | null {
		return mode === 'default' ? null : mode === 'unlimited' ? UNLIMITED : parsed;
	}

	export function show(target: AdminUser, ocrDefaultBudgetYen: number) {
		user = target;
		defaultBudget = ocrDefaultBudgetYen;
		budgetMode = modeFor(target.ocrBudgetYen);
		budget = inputFor(target.ocrBudgetYen);
		submitted = false;
		open = true;
	}

	async function submit(event: SubmitEvent) {
		event.preventDefault();
		if (loading || !user) return;
		submitted = true;
		const parsedBudget = parseBudget(budget);
		if (budgetMode === 'custom' && parsedBudget === null) return;
		loading = true;
		const result = await setUserOcrLimit(user.id, valueFor(budgetMode, parsedBudget));
		loading = false;
		if (!result.ok) {
			errorDialog?.show(m.admin_users_ocr_limit_error_title(), result.message);
			return;
		}
		open = false;
		onsaved();
	}
</script>

<Dialog.Root bind:open>
	<Dialog.Content bind:ref={content} {...closeGuard} onOpenAutoFocus={focusOnOpen}>
		{#if user}
			<DialogFormHeader
				title={m.admin_users_ocr_limit_dialog_title()}
				description={m.admin_users_ocr_limit_dialog_description({ name: userDisplayName(user) })}
				closeDisabled={loading}
			/>
			<form onsubmit={submit}>
				<Field.FieldGroup class="gap-3.5">
					<Field.Field>
						<Field.FieldLabel for="ocr-budget-mode">
							{m.admin_users_ocr_budget_mode_label()}
						</Field.FieldLabel>
						<!-- ADR: 設定画面のタイムゾーンと同じくネイティブの select を使う。選択肢が3つで
						     入れ子の状態を持たず、モバイルではOS標準のピッカーの方が押しやすいため。 -->
						<select
							id="ocr-budget-mode"
							class={[FIELD_SELECT_CLASS, 'w-full']}
							bind:value={budgetMode}
						>
							<option value="default">{defaultBudgetModeLabel}</option>
							<option value="unlimited">{m.admin_users_ocr_budget_mode_unlimited()}</option>
							<option value="custom">{m.admin_users_ocr_budget_mode_custom()}</option>
						</select>
					</Field.Field>
					{#if budgetMode === 'custom'}
						<Field.Field>
							<Field.FieldLabel for="ocr-budget-yen">
								{m.admin_users_ocr_budget_label()}<RequiredMark />
							</Field.FieldLabel>
							<Input
								id="ocr-budget-yen"
								type="text"
								inputmode="numeric"
								{@attach halfWidthInput}
								required
								aria-invalid={budgetError !== undefined}
								aria-describedby={budgetError ? 'ocr-budget-yen-error' : undefined}
								bind:value={budget}
							/>
							<FieldErrorList id="ocr-budget-yen-error" messages={[budgetError]} />
						</Field.Field>
					{/if}
				</Field.FieldGroup>
				<DialogFormFooter {loading} oncancel={() => (open = false)}>
					<LoadingButton type="submit" {loading}>
						{m.admin_users_ocr_limit_submit_button()}
					</LoadingButton>
				</DialogFormFooter>
			</form>
		{/if}
	</Dialog.Content>
</Dialog.Root>

<NoticeDialog bind:this={errorDialog} />
