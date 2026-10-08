import { afterEach, describe, expect, it, vi } from 'vitest';
import { isInAppBrowser } from '$lib/in-app-browser';

afterEach(() => {
	vi.unstubAllGlobals();
});

function withUserAgent(userAgent: string) {
	vi.stubGlobal('navigator', { userAgent });
}

describe('isInAppBrowser', () => {
	it('LINE・Instagram・Facebook の内蔵ブラウザなら true', () => {
		withUserAgent(
			'Mozilla/5.0 (iPhone; CPU iPhone OS 26_6 like Mac OS X) Safari/605.1.15 Line/15.0.0'
		);
		expect(isInAppBrowser()).toBe(true);
		withUserAgent(
			'Mozilla/5.0 (Linux; Android 15; wv) Chrome/140 Mobile Safari/537.36 Line/15.0.0'
		);
		expect(isInAppBrowser()).toBe(true);
		withUserAgent('Mozilla/5.0 (iPhone) Mobile/15E148 Instagram 400.0.0');
		expect(isInAppBrowser()).toBe(true);
		withUserAgent('Mozilla/5.0 (iPhone) Mobile/15E148 [FBAN/FBIOS;FBAV/500.0]');
		expect(isInAppBrowser()).toBe(true);
	});

	it('通常のブラウザでは false', () => {
		withUserAgent(
			'Mozilla/5.0 (iPhone; CPU iPhone OS 26_6 like Mac OS X) Version/26.0 Safari/605.1.15'
		);
		expect(isInAppBrowser()).toBe(false);
		withUserAgent('Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) Chrome/140 Safari/537.36');
		expect(isInAppBrowser()).toBe(false);
	});

	it('navigator が無い環境 (SSR・ビルド時) でも落ちずに false', () => {
		vi.stubGlobal('navigator', undefined);
		expect(isInAppBrowser()).toBe(false);
	});
});
