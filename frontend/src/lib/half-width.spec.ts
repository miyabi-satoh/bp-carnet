import { describe, expect, it } from 'vitest';
import { toHalfWidth } from '$lib/half-width';

describe('toHalfWidth', () => {
	it('全角の数字・英字・記号を半角にする', () => {
		expect(toHalfWidth('１２０')).toBe('120');
		expect(toHalfWidth('ｔａｒｏ＠ｅｘａｍｐｌｅ．ｃｏｍ')).toBe('taro@example.com');
		expect(toHalfWidth('－１')).toBe('-1');
	});

	it('全角スペースを半角にする', () => {
		expect(toHalfWidth('１２０\u3000')).toBe('120 ');
	});

	it('日本語や半角の文字は変えない', () => {
		expect(toHalfWidth('朝の薬 ㈱ 120')).toBe('朝の薬 ㈱ 120');
	});
});
