import { describe, expect, it } from 'vitest';
import ja from '../../messages/ja.json';

/** 単位として数の後に来る語。プレースホルダーは名前で数かどうかを決めず、後ろの語で見る */
const UNIT = '(?:秒|分|時間|日|週|か月|年|件|回|文字|行|個|つ|枚|円|人|[KMGT]B)';
/** 数字の後に日本語か単位、またはプレースホルダーの後に単位が、ふつうの空白を挟んで続く所 */
const BREAKABLE_NUMBER_UNIT = new RegExp(
	`\\d (?=[一-龠々ぁ-んァ-ヶ]|${UNIT})|\\{\\w+\\} (?=${UNIT})`
);

/** 画面に出す Markdown の本文 (規約類・使い方)。HTML のコメントは画面に出ないので外して見る */
const MARKDOWN_BODIES = import.meta.glob<string>(['./legal/ja/*.md', './help/ja/*.md'], {
	query: '?raw',
	import: 'default',
	eager: true
});

describe('日本語の文言', () => {
	// 数字と単位の間は、そこで折り返さないようノーブレークスペースにする
	it('数字と単位の間に、ふつうの空白を置かない (messages)', () => {
		const breakable = Object.entries(ja)
			.filter(([key, value]) => key !== '$schema' && BREAKABLE_NUMBER_UNIT.test(String(value)))
			.map(([key]) => key);
		expect(breakable).toEqual([]);
	});

	it('数字と単位の間に、ふつうの空白を置かない (規約類・使い方の本文)', () => {
		expect(Object.keys(MARKDOWN_BODIES).length).toBeGreaterThan(0);
		const breakable = Object.entries(MARKDOWN_BODIES).flatMap(([path, body]) =>
			body
				.replace(/<!--[\s\S]*?-->/g, '')
				.split('\n')
				.filter((line) => BREAKABLE_NUMBER_UNIT.test(line))
				.map((line) => `${path}: ${line}`)
		);
		expect(breakable).toEqual([]);
	});
});
