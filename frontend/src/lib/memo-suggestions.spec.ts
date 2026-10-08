import { describe, expect, it } from 'vitest';
import {
	applyMemoSuggestion,
	memoPhrases,
	memoSuggestions,
	rankMemoPhrases
} from './memo-suggestions';

function record(memo: string, localMeasuredAt: string) {
	return { memo, localMeasuredAt };
}

describe('memoPhrases', () => {
	it('splits by 、 and drops blanks', () => {
		expect(memoPhrases(' 起床後、 薬を飲む前 、、')).toEqual(['起床後', '薬を飲む前']);
		expect(memoPhrases('')).toEqual([]);
	});
});

describe('rankMemoPhrases', () => {
	it('orders by count, then by the latest use', () => {
		const ranked = rankMemoPhrases([
			record('起床後、薬を飲む前', '2026-09-01T07:00'),
			record('起床後', '2026-09-02T07:00'),
			record('頭痛', '2026-09-03T21:00'),
			record('寝る前', '2026-09-04T22:00'),
			record('', '2026-09-05T07:00')
		]);
		expect(ranked).toEqual(['起床後', '寝る前', '頭痛', '薬を飲む前']);
	});

	it('counts a phrase once per record', () => {
		expect(
			rankMemoPhrases([
				record('頭痛、頭痛', '2026-09-01T07:00'),
				record('寝る前', '2026-09-02T07:00'),
				record('寝る前', '2026-09-03T07:00')
			])
		).toEqual(['寝る前', '頭痛']);
	});
});

describe('memoSuggestions', () => {
	const ranked = ['起床後', '薬を飲む前', '頭痛', '薬を飲んだ後', '寝る前', '運動の後'];

	it('shows the most used phrases up to the limit when the memo is empty', () => {
		expect(memoSuggestions(ranked, '')).toEqual(ranked.slice(0, 5));
	});

	it('leaves out phrases already in the memo', () => {
		expect(memoSuggestions(ranked, '起床後、頭痛', 3)).toEqual([
			'薬を飲む前',
			'薬を飲んだ後',
			'寝る前'
		]);
	});

	it('narrows to phrases containing what is being typed', () => {
		expect(memoSuggestions(ranked, '起床後、薬')).toEqual(['薬を飲む前', '薬を飲んだ後']);
		expect(memoSuggestions(ranked, '後')).toEqual(['起床後', '薬を飲んだ後', '運動の後']);
		expect(memoSuggestions(ranked, 'めまい')).toEqual([]);
	});

	it('treats a phrase contained in another unused phrase as being typed', () => {
		const withLonger = ['頭痛', '起床後', '頭痛薬'];
		expect(memoSuggestions(withLonger, '頭痛')).toEqual(['頭痛薬']);
		// 長いほうがもうメモに入っていれば、打ち終えた言葉とみなす。
		expect(memoSuggestions(withLonger, '頭痛薬、頭痛')).toEqual(['起床後']);
	});
});

describe('applyMemoSuggestion', () => {
	const ranked = ['起床後', '薬を飲む前', '頭痛'];

	it('puts the phrase into an empty memo', () => {
		expect(applyMemoSuggestion(ranked, '', '頭痛')).toBe('頭痛');
	});

	it('appends after a finished phrase with 、', () => {
		expect(applyMemoSuggestion(ranked, '起床後', '頭痛')).toBe('起床後、頭痛');
		expect(applyMemoSuggestion(ranked, '起床後、', '頭痛')).toBe('起床後、頭痛');
	});

	it('replaces what is being typed', () => {
		expect(applyMemoSuggestion(ranked, '起床後、薬', '薬を飲む前')).toBe('起床後、薬を飲む前');
		expect(applyMemoSuggestion(ranked, '頭', '頭痛')).toBe('頭痛');
		expect(applyMemoSuggestion([...ranked, '頭痛薬'], '起床後、頭痛', '頭痛薬')).toBe(
			'起床後、頭痛薬'
		);
	});
});
