import { render } from 'svelte/server';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { legalDocumentComponent, type LegalDocument } from '$lib/legal';

const DOCUMENTS: readonly LegalDocument[] = ['terms', 'privacy', 'tokushoho'];

describe('legalDocumentComponent', () => {
	it('本文のリンクは、URL の後ろの文まで取り込まず、nofollow も付けない', () => {
		for (const document of DOCUMENTS) {
			const { body } = render(legalDocumentComponent(document, 'ja'));
			const links = [...body.matchAll(/<a [^>]*>/g)].map(([tag]) => tag);
			expect(links.length, document).toBeGreaterThan(0);
			for (const link of links) {
				// 裸の URL の自動リンクは、続く全角の句読点や日本語まで URL に取り込む (UTF-8 の多バイト文字としてエンコードされる)。
				expect(link, document).not.toMatch(/href="[^"]*%[C-F][0-9A-F]/i);
				expect(link, document).not.toContain('nofollow');
			}
		}
	});
});

describe('特商法の販売価格', () => {
	afterEach(() => {
		vi.useRealTimers();
	});

	it('インボイスの登録の日から、免税事業者の表示を税込みに切り替える', () => {
		vi.useFakeTimers();
		vi.setSystemTime(new Date('2026-11-30T23:59:59+09:00'));
		expect(render(legalDocumentComponent('tokushoho', 'ja')).body).toContain('免税事業者');

		vi.setSystemTime(new Date('2026-12-01T00:00:00+09:00'));
		const { body } = render(legalDocumentComponent('tokushoho', 'ja'));
		expect(body).toContain('300\u00a0円（税込み）');
		expect(body).not.toContain('免税事業者');
	});
});
