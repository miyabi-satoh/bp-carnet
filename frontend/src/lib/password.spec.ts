import { describe, expect, it } from 'vitest';
import * as m from '$lib/paraglide/messages.js';
import {
	isNewPasswordValid,
	newPasswordErrors,
	passwordPolicyError,
	passwordPolicyHint,
	type PasswordPolicy
} from '$lib/password';

const lenient: PasswordPolicy = { minLength: 4, requiredClasses: [] };

describe('passwordPolicyError', () => {
	it('最小長を満たせばエラーにしない', () => {
		expect(passwordPolicyError(lenient, '1234')).toBeUndefined();
	});

	it('最小長に足りなければ文字数を伝える', () => {
		expect(passwordPolicyError(lenient, '123')).toBe(
			m.password_policy_too_short_error({ minLength: 4 })
		);
	});

	it('文字数はコードポイントで数える', () => {
		expect(passwordPolicyError(lenient, 'あいうえ')).toBeUndefined();
	});

	it('不足している文字種だけを並べる', () => {
		const policy: PasswordPolicy = {
			minLength: 4,
			requiredClasses: ['digit', 'uppercase', 'symbol']
		};
		expect(passwordPolicyError(policy, 'Pass1')).toBe(
			m.password_policy_missing_classes_error({ classes: m.password_class_symbol() })
		);
		expect(passwordPolicyError(policy, 'Pass1!')).toBeUndefined();
	});

	it('文字種の判定は ASCII 基準 (ひらがなは英小文字ではなく記号)', () => {
		expect(passwordPolicyError({ minLength: 1, requiredClasses: ['lowercase'] }, 'ひらがな')).toBe(
			m.password_policy_missing_classes_error({ classes: m.password_class_lowercase() })
		);
		expect(
			passwordPolicyError({ minLength: 1, requiredClasses: ['symbol'] }, 'ひらがな')
		).toBeUndefined();
	});
});

describe('newPasswordErrors', () => {
	it('ポリシー違反と不一致をそれぞれの欄に返す', () => {
		expect(newPasswordErrors(lenient, '123', '12')).toEqual({
			password: m.password_policy_too_short_error({ minLength: 4 }),
			confirmation: m.common_password_mismatch_error()
		});
	});

	it('2欄とも問題が無ければどちらも undefined', () => {
		expect(newPasswordErrors(lenient, '1234', '1234')).toEqual({
			password: undefined,
			confirmation: undefined
		});
	});
});

describe('isNewPasswordValid', () => {
	it('どちらかの欄にエラーがあれば false', () => {
		expect(isNewPasswordValid(lenient, '123', '123')).toBe(false);
		expect(isNewPasswordValid(lenient, '1234', '12345')).toBe(false);
		expect(isNewPasswordValid(lenient, '1234', '1234')).toBe(true);
	});
});

describe('passwordPolicyHint', () => {
	it('文字種の指定が無ければ文字数だけを示す', () => {
		expect(passwordPolicyHint(lenient)).toBe(m.password_policy_hint({ minLength: 4 }));
	});

	it('文字種の指定があれば併せて示す', () => {
		expect(passwordPolicyHint({ minLength: 8, requiredClasses: ['digit', 'lowercase'] })).toBe(
			m.password_policy_hint_with_classes({
				minLength: 8,
				classes: `${m.password_class_digit()}${m.common_list_separator()}${m.password_class_lowercase()}`
			})
		);
	});
});
