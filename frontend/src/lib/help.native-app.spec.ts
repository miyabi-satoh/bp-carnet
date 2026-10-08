// アプリ (Capacitor) の中で開いたときの使い方の項目 (App Review Guidelines 2.3.10)。
import { describe, expect, it, vi } from 'vitest';

vi.mock('$lib/native-app', () => ({ isNativeApp: true }));

import { errorHelpSlug } from '$lib/api/errors';
import { findHelpTopic, HELP_TOPICS, loadHelpTopicContent } from './help';

describe('使い方の項目 (アプリ)', () => {
	it('アプリを入れる・ホーム画面に追加・内蔵ブラウザの項目を出さず、エラーの案内からもリンクしない', () => {
		const slugs = HELP_TOPICS.map((topic) => topic.slug);
		expect(slugs).not.toContain('iphone-app');
		expect(slugs).not.toContain('home-screen-android');
		expect(slugs).not.toContain('trouble-in-app-browser');
		expect(findHelpTopic('iphone-app')).toBeUndefined();

		expect(errorHelpSlug('google_failed')).toBeUndefined();
		expect(errorHelpSlug('line_email_required')).toBe('trouble-line-email');
	});
});

describe('使い方の本文 (アプリ)', () => {
	// 本文の Markdown は初めて読み込むときにコンパイルされ、ほかのテストと並ぶと既定の 5 秒を超えるので延ばす (→ help.spec.ts)。
	it(
		'買い足しの項目は、アプリの手順 (アプリ内課金) の本文を読む',
		{ timeout: 30_000 },
		async () => {
			const topic = findHelpTopic('trouble-ocr-budget');
			expect(topic).toBeDefined();
			const [app, web] = await Promise.all([
				loadHelpTopicContent(topic!),
				loadHelpTopicContent(topic!, undefined, false)
			]);
			expect(app).not.toBe(web);
		}
	);
});
