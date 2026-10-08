import { afterEach, describe, expect, it, vi } from 'vitest';
import { isIosWebApp } from '$lib/ios-web-app';

afterEach(() => {
	vi.unstubAllGlobals();
});

describe('isIosWebApp', () => {
	it('navigator.standalone が true のときだけ true', () => {
		vi.stubGlobal('navigator', { standalone: true });
		expect(isIosWebApp()).toBe(true);
		vi.stubGlobal('navigator', { standalone: false });
		expect(isIosWebApp()).toBe(false);
	});

	it('standalone が無いブラウザでは false', () => {
		vi.stubGlobal('navigator', {});
		expect(isIosWebApp()).toBe(false);
	});

	it('display-mode: standalone だけでは true にしない', () => {
		vi.stubGlobal('navigator', {});
		vi.stubGlobal('matchMedia', () => ({ matches: true }));
		expect(isIosWebApp()).toBe(false);
	});

	it('navigator が無い環境 (SSR・ビルド時) でも落ちずに false', () => {
		vi.stubGlobal('navigator', undefined);
		expect(isIosWebApp()).toBe(false);
	});
});
