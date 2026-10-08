<script lang="ts">
	import * as Field from '$lib/components/ui/field';
	import FieldErrorList from '$lib/components/field-error-list.svelte';
	import PasswordInput from '$lib/components/password-input.svelte';
	import { newPasswordErrors, passwordPolicyHint, type PasswordPolicy } from '$lib/password';
	import RequiredMark from '$lib/components/required-mark.svelte';

	type Props = {
		policy: PasswordPolicy;
		passwordLabel: string;
		confirmLabel: string;
		/** 欄の id の接頭辞。同じ画面に置く他の欄と衝突しないように呼び出し側で決める。 */
		idPrefix: string;
		password: string;
		confirmation: string;
		/** 入力中の欄にエラーを出し続けないよう、一度送信するまでは欄の下に何も出さない。 */
		submitted: boolean;
	};

	let {
		policy,
		passwordLabel,
		confirmLabel,
		idPrefix,
		password = $bindable(),
		confirmation = $bindable(),
		submitted
	}: Props = $props();

	let errors = $derived(submitted ? newPasswordErrors(policy, password, confirmation) : undefined);
</script>

<!-- 新しいパスワードと確認の2欄。フォームの枠・送信ボタンは画面ごとに違うため呼び出し側に置く。
     `minlength` は新しいパスワードの欄だけに付ける (確認欄は一致しているかが問題で、長さは元の欄で止まる)。
     文字数の数え方はブラウザが UTF-16 の単位、`newPasswordErrors` が文字単位なので、境目の判定は自前のエラーが正となる。 -->
<Field.Field>
	<Field.FieldLabel for="{idPrefix}-new">{passwordLabel}<RequiredMark /></Field.FieldLabel>
	<PasswordInput
		id="{idPrefix}-new"
		autocomplete="new-password"
		aria-invalid={errors?.password !== undefined}
		aria-describedby={errors?.password ? `${idPrefix}-hint ${idPrefix}-error` : `${idPrefix}-hint`}
		bind:value={password}
		required
		minlength={policy.minLength}
	/>
	<Field.FieldDescription id="{idPrefix}-hint">{passwordPolicyHint(policy)}</Field.FieldDescription>
	<FieldErrorList id="{idPrefix}-error" messages={[errors?.password]} />
</Field.Field>
<Field.Field>
	<Field.FieldLabel for="{idPrefix}-confirm">{confirmLabel}<RequiredMark /></Field.FieldLabel>
	<PasswordInput
		id="{idPrefix}-confirm"
		autocomplete="new-password"
		aria-invalid={errors?.confirmation !== undefined}
		aria-describedby={errors?.confirmation ? `${idPrefix}-confirm-error` : undefined}
		bind:value={confirmation}
		required
	/>
	<FieldErrorList id="{idPrefix}-confirm-error" messages={[errors?.confirmation]} />
</Field.Field>
