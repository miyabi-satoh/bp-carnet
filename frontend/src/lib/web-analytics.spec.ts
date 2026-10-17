import { describe, expect, it } from 'vitest';
import { isAnalyticsRoute } from './web-analytics';

describe('数える画面', () => {
	it('ログインしていない画面を数え、メールのリンクから開く画面とログインした後の画面は数えない', () => {
		expect(isAnalyticsRoute('/login')).toBe(true);
		expect(isAnalyticsRoute('/help/[slug]')).toBe(true);
		expect(isAnalyticsRoute('/reset-password')).toBe(false);
		expect(isAnalyticsRoute('/verify-email')).toBe(false);
		expect(isAnalyticsRoute('/change-email')).toBe(false);
		expect(isAnalyticsRoute('/')).toBe(false);
		expect(isAnalyticsRoute(null)).toBe(false);
	});
});
