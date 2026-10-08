import { describe, expect, it } from 'vitest';
import { pageTitle } from '$lib/page-title';

describe('pageTitle', () => {
	it('公開ページはページ別の title にする', () => {
		expect(pageTitle('/terms')).toBe('利用規約 — BP Carnet');
		expect(pageTitle('/privacy')).toBe('プライバシーポリシー — BP Carnet');
		expect(pageTitle('/pricing')).toBe('料金と購入の条件 — BP Carnet');
		expect(pageTitle('/licenses')).toBe('第三者のソフトウェア — BP Carnet');
		expect(pageTitle('/help')).toBe('使い方 — BP Carnet');
		expect(pageTitle('/help/[slug]', 'login')).toBe('ログインする — 使い方 — BP Carnet');
	});

	it('トップと /login は同じ title にする', () => {
		expect(pageTitle('/login')).toBe(pageTitle('/'));
		expect(pageTitle('/')).toBe('BP Carnet — 血圧の記録を、写真を撮るだけで');
	});

	it('それ以外や、無い項目は BP Carnet にする', () => {
		expect(pageTitle('/settings')).toBe('BP Carnet');
		expect(pageTitle('/signup')).toBe('BP Carnet');
		expect(pageTitle(null)).toBe('BP Carnet');
		expect(pageTitle('/help/[slug]', 'nope')).toBe('BP Carnet');
	});
});
