import { describe, expect, it } from 'vitest';
import { isTaxIncluded } from '$lib/tax';

describe('isTaxIncluded', () => {
	it('日本時間の 2026-12-01 0時から税込みになる', () => {
		expect(isTaxIncluded(new Date('2026-11-30T23:59:59+09:00'))).toBe(false);
		expect(isTaxIncluded(new Date('2026-12-01T00:00:00+09:00'))).toBe(true);
	});
});
