import { describe, expect, it, vi } from 'vitest';

vi.mock('$app/state', () => ({ page: { data: {} } }));
vi.mock('$lib/api/client', () => ({ client: {} }));
import { canUnlinkIdentity } from './auth';

describe('canUnlinkIdentity', () => {
	it.each([
		{ passwordUsable: true, linkedProviders: ['google'] as const, expected: true },
		{ passwordUsable: true, linkedProviders: ['line', 'google'] as const, expected: true },
		{ passwordUsable: false, linkedProviders: ['line', 'google'] as const, expected: true },
		// 最後のログイン方法 (パスワードも別の連携も無い) は外せない。
		{ passwordUsable: false, linkedProviders: ['google'] as const, expected: false },
		{ passwordUsable: false, linkedProviders: ['line'] as const, expected: false }
	])(
		'パスワード $passwordUsable・連携 $linkedProviders のとき $expected',
		({ passwordUsable, linkedProviders, expected }) => {
			expect(canUnlinkIdentity({ passwordUsable, linkedProviders })).toBe(expected);
		}
	);
});
