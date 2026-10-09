import { beforeEach, describe, expect, it, vi } from 'vitest';
import type { OcrStatus } from '$lib/ocr';

vi.mock('$lib/ocr', () => ({ fetchOcrStatus: vi.fn() }));
import { fetchOcrStatus } from '$lib/ocr';
import { OcrStatusState } from './ocr-status.svelte';

function status(changes: Partial<OcrStatus> = {}): OcrStatus {
	return {
		enabled: true,
		quotas: null,
		quotaLow: false,
		topupAvailable: false,
		consented: true,
		...changes
	};
}

beforeEach(() => {
	vi.mocked(fetchOcrStatus).mockReset();
});

describe('OcrStatusState', () => {
	it('keepKnownConsent を付けると、取り直しで同意が分からなくなっても前に分かった同意を残す', async () => {
		const ocr = new OcrStatusState({ keepKnownConsent: true });
		vi.mocked(fetchOcrStatus).mockResolvedValueOnce(status({ consented: true }));
		await ocr.refresh();

		vi.mocked(fetchOcrStatus).mockResolvedValueOnce(status({ enabled: null, consented: null }));
		await ocr.refresh();

		expect(ocr.status.enabled).toBe(null);
		expect(ocr.status.consented).toBe(true);
	});

	it('keepKnownConsent を付けなければ、取り直しで分からなくなった同意は null にする', async () => {
		const ocr = new OcrStatusState();
		vi.mocked(fetchOcrStatus).mockResolvedValueOnce(status({ consented: true }));
		await ocr.refresh();

		vi.mocked(fetchOcrStatus).mockResolvedValueOnce(status({ enabled: null, consented: null }));
		await ocr.refresh();

		expect(ocr.status.consented).toBe(null);
	});

	it('先に始めた取り直しが遅れて返っても、新しい値を上書きせず false を返す', async () => {
		const ocr = new OcrStatusState();
		let finishFirst: (value: OcrStatus) => void = () => {};
		vi.mocked(fetchOcrStatus)
			.mockImplementationOnce(() => new Promise((resolve) => (finishFirst = resolve)))
			.mockResolvedValueOnce(status({ quotaLow: true }));

		const first = ocr.refresh();
		expect(await ocr.refresh()).toBe(true);
		finishFirst(status({ quotaLow: false }));

		expect(await first).toBe(false);
		expect(ocr.status.quotaLow).toBe(true);
	});
});
