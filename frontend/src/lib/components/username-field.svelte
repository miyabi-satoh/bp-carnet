<script lang="ts">
	import { halfWidthInput } from '$lib/half-width';
	import * as Field from '$lib/components/ui/field';
	import FieldErrorList from '$lib/components/field-error-list.svelte';
	import { Input } from '$lib/components/ui/input';
	import * as m from '$lib/paraglide/messages.js';
	import { usernameError } from '$lib/username';
	import type { ComponentProps } from 'svelte';
	import RequiredMark from '$lib/components/required-mark.svelte';

	/** 欄の id・値・補完・エラーの関連付けはこのコンポーネントが決めるため、呼び出し側からは受け取らない。 */
	type Props = Omit<
		ComponentProps<typeof Input>,
		'id' | 'value' | 'autocomplete' | 'aria-invalid' | 'aria-describedby' | 'type' | 'files'
	> & {
		/** 欄の id の接頭辞。同じ画面に置く他の欄と衝突しないように呼び出し側で決める。 */
		idPrefix: string;
		/** 本人が使うIDなら `username`、他人のIDを作るなら `off` (ブラウザに自分のIDを補完させない)。 */
		autocomplete: 'username' | 'off';
		value: string;
		/** 入力中の欄にエラーを出し続けないよう、一度送信するまでは欄の下に何も出さない。 */
		submitted: boolean;
	};

	let { idPrefix, autocomplete, value = $bindable(), submitted, ...restProps }: Props = $props();

	let issue = $derived(submitted ? usernameError(value) : undefined);
</script>

<!-- 新しく作るユーザーのID欄。送信可否の判定は呼び出し側で `usernameError` を使う。 -->
<Field.Field>
	<Field.FieldLabel for={idPrefix}>{m.common_username_label()}<RequiredMark /></Field.FieldLabel>
	<Input
		id={idPrefix}
		{autocomplete}
		inputmode="email"
		autocapitalize="off"
		{@attach halfWidthInput}
		required
		aria-invalid={issue !== undefined}
		aria-describedby={issue ? `${idPrefix}-hint ${idPrefix}-error` : `${idPrefix}-hint`}
		bind:value
		{...restProps}
	/>
	<Field.FieldDescription id="{idPrefix}-hint">{m.common_username_hint()}</Field.FieldDescription>
	<FieldErrorList id="{idPrefix}-error" messages={[issue]} />
</Field.Field>
