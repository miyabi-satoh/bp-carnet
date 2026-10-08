import { describe, expect, it } from 'vitest';
import { usernameError } from '$lib/username';

describe('usernameError', () => {
	it('メールアドレスも任意のIDも受け付ける', () => {
		expect(usernameError('kyoko')).toBeUndefined();
		expect(usernameError('hahaue@example.com')).toBeUndefined();
	});

	it('空・空白・制御文字を弾く', () => {
		expect(usernameError('')).toBeDefined();
		expect(usernameError('with space')).toBeDefined();
		expect(usernameError('tab\there')).toBeDefined();
		expect(usernameError('null\u0000char')).toBeDefined();
	});

	it('見た目で気づけない書式文字 (Cf) も弾く', () => {
		expect(usernameError('ky\ufeffoko')).toBeDefined(); // ZERO WIDTH NO-BREAK SPACE
		expect(usernameError('a\u200bb')).toBeDefined(); // ZERO WIDTH SPACE
		expect(usernameError('a\u200cb')).toBeDefined(); // ZERO WIDTH NON-JOINER
		expect(usernameError('a\u200db')).toBeDefined(); // ZERO WIDTH JOINER
		expect(usernameError('a\u2060b')).toBeDefined(); // WORD JOINER
	});

	it('日本語のユーザーIDは使える (弾くのは見えない文字だけ)', () => {
		expect(usernameError('きょうこ')).toBeUndefined();
	});

	it('254文字までは通し、超えたら弾く', () => {
		expect(usernameError('a'.repeat(254))).toBeUndefined();
		expect(usernameError('a'.repeat(255))).toBeDefined();
	});

	it('サロゲートペアは1文字として数える', () => {
		// `.length` で数えると絵文字254個は508になり、上限ちょうどを通せない。
		expect(usernameError('😀'.repeat(254))).toBeUndefined();
		expect(usernameError('😀'.repeat(255))).toBeDefined();
	});
});
