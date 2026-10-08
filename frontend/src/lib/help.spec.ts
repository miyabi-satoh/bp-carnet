import { existsSync, readdirSync } from 'node:fs';
import path from 'node:path';
import { render } from 'svelte/server';
import { describe, expect, it } from 'vitest';
import {
	adjacentHelpTopics,
	findHelpTopic,
	HELP_SECTIONS,
	HELP_TOPICS,
	loadHelpTopicContent
} from '$lib/help';
import HelpContent from './help.test-harness.svelte';

const STATIC_DIR = path.join(import.meta.dirname, '..', '..', 'static');

describe('HELP_SECTIONS', () => {
	it('項目の slug は重ならない', () => {
		const slugs = HELP_TOPICS.map((topic) => topic.slug);
		expect(new Set(slugs).size).toBe(slugs.length);
	});

	it('どの項目も本文があり、本文だけの項目は無い', () => {
		const files = readdirSync(path.join(import.meta.dirname, 'help', 'ja'));
		expect(files.sort()).toEqual(
			HELP_TOPICS.flatMap((topic) =>
				topic.appBody ? [`${topic.slug}.md`, `${topic.slug}.app.md`] : [`${topic.slug}.md`]
			).sort()
		);
	});

	// 本文の Markdown は初めて読み込むときにコンパイルされ、ほかのテストと並んで走ると既定の 5 秒を超えるので、
	// まとめて並べて読み込み、時間切れも延ばす。
	it(
		'どの項目も手順は1から順に番号の付いた3〜5歩で、画像を添える歩の画像が static にある',
		{ timeout: 30_000 },
		async () => {
			// アプリの本文 (`appBody`) も、ウェブの本文と同じ決まりで確かめる。
			const bodies = [
				...HELP_TOPICS.map((topic) => ({ topic, app: false })),
				...HELP_TOPICS.filter((topic) => topic.appBody).map((topic) => ({ topic, app: true }))
			];
			const contents = await Promise.all(
				bodies.map(({ topic, app }) => loadHelpTopicContent(topic, undefined, app))
			);
			for (const [i, { topic }] of bodies.entries()) {
				const Content = contents[i];
				const { body } = render(HelpContent, { props: { topic, Content } });
				const numbers = [...body.matchAll(/aria-hidden="true">(\d+)<\/span>/g)].map(([, n]) =>
					Number(n)
				);
				expect(numbers.length, topic.slug).toBeGreaterThanOrEqual(3);
				expect(numbers.length, topic.slug).toBeLessThanOrEqual(5);
				expect(numbers, topic.slug).toEqual(numbers.map((_, i) => i + 1));
				expect(body.match(/<li\b/g)?.length, topic.slug).toBe(numbers.length);
				for (const [, src] of body.matchAll(/<img src="([^"]+)"/g)) {
					expect(src, topic.slug).toMatch(new RegExp(`^/help/${topic.slug}-[1-5]\\.webp$`));
					expect(existsSync(path.join(STATIC_DIR, src)), src).toBe(true);
				}
			}
		}
	);

	it('項目は目次の並びの順に並ぶ', () => {
		expect(HELP_TOPICS).toEqual(HELP_SECTIONS.flatMap((section) => section.topics));
	});
});

describe('adjacentHelpTopics', () => {
	it('前後の項目を目次の順で返し、端では無い', () => {
		const first = HELP_TOPICS[0];
		const last = HELP_TOPICS[HELP_TOPICS.length - 1];
		expect(adjacentHelpTopics(first)).toEqual({ prev: undefined, next: HELP_TOPICS[1] });
		expect(adjacentHelpTopics(last)).toEqual({
			prev: HELP_TOPICS[HELP_TOPICS.length - 2],
			next: undefined
		});
	});
});

describe('findHelpTopic', () => {
	it('無い slug では undefined', () => {
		expect(findHelpTopic(HELP_TOPICS[0].slug)).toBe(HELP_TOPICS[0]);
		expect(findHelpTopic('no-such-topic')).toBeUndefined();
	});
});
