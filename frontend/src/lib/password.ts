import { client } from '$lib/api/client';
import { type ApiResult, unwrapRequest } from '$lib/api/errors';
import type { components } from '$lib/api/schema';
import * as m from '$lib/paraglide/messages.js';

/** 設定 (`[password]`) のパスワードポリシー。`GET /api/v1/auth/password-policy` の応答。 */
export type PasswordPolicy = components['schemas']['PasswordPolicyResponse'];

/** `GET /api/v1/auth/password-policy` でポリシーを取得する (`fetchSettings` と同じ形)。
 * 条件はサーバーの設定で決まるため、入力欄に条件を示すには問い合わせが要る。 */
export function fetchPasswordPolicy(): Promise<ApiResult<{ data: PasswordPolicy }>> {
	return unwrapRequest(client.GET('/api/v1/auth/password-policy'));
}

type CharClass = components['schemas']['CharClass'];

const CLASS_LABELS: Record<CharClass, () => string> = {
	digit: m.password_class_digit,
	lowercase: m.password_class_lowercase,
	uppercase: m.password_class_uppercase,
	symbol: m.password_class_symbol
};

/** 判定の基準はバックエンド (`config::CharClass::contained_in`) と同じく ASCII。Unicode の
 * 大小文字判定だと、ひらがなが「英小文字」に該当してしまう。 */
const CLASS_PATTERNS: Record<CharClass, RegExp> = {
	digit: /[0-9]/,
	lowercase: /[a-z]/,
	uppercase: /[A-Z]/,
	symbol: /[^0-9A-Za-z]/
};

function joinClasses(classes: CharClass[]): string {
	return classes.map((name) => CLASS_LABELS[name]()).join(m.common_list_separator());
}

/** 入力欄に添える条件の説明。送信して初めて条件を知らせるのを避けるため、常に出す。 */
export function passwordPolicyHint(policy: PasswordPolicy): string {
	const minLength = policy.minLength;
	return policy.requiredClasses.length === 0
		? m.password_policy_hint({ minLength })
		: m.password_policy_hint_with_classes({
				minLength,
				classes: joinClasses(policy.requiredClasses)
			});
}

/** ポリシー違反の文言。規則はバックエンド (`auth::validate_password`) と同じで、判定も
 * サーバーが改めて行う (こちらは送信前に欄の直下で知らせるためのもの)。
 *
 * 文字数はコードポイントで数える (バックエンドと同じ基準)。 */
export function passwordPolicyError(policy: PasswordPolicy, password: string): string | undefined {
	if ([...password].length < policy.minLength) {
		return m.password_policy_too_short_error({ minLength: policy.minLength });
	}
	const missing = policy.requiredClasses.filter((name) => !CLASS_PATTERNS[name].test(password));
	if (missing.length > 0) {
		return m.password_policy_missing_classes_error({ classes: joinClasses(missing) });
	}
	return undefined;
}

/** 新しいパスワードと確認の2欄それぞれのエラー。欄の表示 (`new-password-fields.svelte`) と
 * 送信前の判定の両方で使う。 */
export type NewPasswordErrors = {
	password: string | undefined;
	confirmation: string | undefined;
};

export function newPasswordErrors(
	policy: PasswordPolicy,
	password: string,
	confirmation: string
): NewPasswordErrors {
	return {
		password: passwordPolicyError(policy, password),
		confirmation: confirmation !== password ? m.common_password_mismatch_error() : undefined
	};
}

/** 2欄とも問題が無いか。送信前に止めるかどうかの判定に使う。 */
export function isNewPasswordValid(
	policy: PasswordPolicy,
	password: string,
	confirmation: string
): boolean {
	const errors = newPasswordErrors(policy, password, confirmation);
	return errors.password === undefined && errors.confirmation === undefined;
}
