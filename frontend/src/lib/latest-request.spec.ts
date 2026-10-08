import { describe, expect, it } from 'vitest';
import { LatestRequest } from './latest-request';

describe('LatestRequest', () => {
	it('新しく始めた処理だけを最新と判定する', () => {
		const requests = new LatestRequest();
		const first = requests.begin();
		const second = requests.begin();

		expect(first()).toBe(false);
		expect(second()).toBe(true);
	});

	it('cancel は進行中の処理を無効にする', () => {
		const requests = new LatestRequest();
		const current = requests.begin();

		requests.cancel();

		expect(current()).toBe(false);
	});
});
