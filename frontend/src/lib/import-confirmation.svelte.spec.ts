import { beforeEach, describe, expect, it, vi } from 'vitest';

vi.mock('$lib/import', async (importOriginal) => ({
	...(await importOriginal<typeof import('$lib/import')>()),
	importRows: vi.fn()
}));
import { importRows, type ImportOutcome, type ImportRow } from '$lib/import';
import * as m from '$lib/paraglide/messages.js';
import { ImportConfirmation } from './import-confirmation.svelte';

/** `importRows` は差し替えているので、行の中身は使われない。 */
const rows: ImportRow[] = [];

/** 完了を外から決められる取り込み。 */
function pendingImport() {
	let finish: (outcome: ImportOutcome) => void = () => {};
	vi.mocked(importRows).mockImplementation(
		() =>
			new Promise((resolve) => {
				finish = resolve;
			})
	);
	return (outcome: ImportOutcome) => finish(outcome);
}

beforeEach(() => {
	vi.mocked(importRows).mockReset();
});

describe('ImportConfirmation', () => {
	it('成功したら登録した件数を渡して onImported を呼び、閉じ終わるまで取り込み中のままにする', async () => {
		vi.mocked(importRows).mockResolvedValue({ ok: true, deleted: 0, created: 2 });
		const onImported = vi.fn();
		const confirmation = new ImportConfirmation(onImported);

		await confirmation.confirm(rows, []);

		expect(onImported).toHaveBeenCalledTimes(1);
		expect(onImported).toHaveBeenCalledWith(2);
		expect(confirmation.importing).toBe(true);
		expect(confirmation.error).toBe('');
	});

	it('失敗したら文言を出し、もう一度確定できるようにする', async () => {
		vi.mocked(importRows).mockResolvedValue({ ok: false, message: '取り込めませんでした' });
		const onImported = vi.fn();
		const confirmation = new ImportConfirmation(onImported);

		await confirmation.confirm(rows, []);

		expect(onImported).not.toHaveBeenCalled();
		expect(confirmation.importing).toBe(false);
		expect(confirmation.error).toBe('取り込めませんでした');
	});

	it('ほかで記録が変わっていたら、エラーではなく案内を出し、既存の記録を取り直させる', async () => {
		vi.mocked(importRows).mockResolvedValue({
			ok: false,
			message: 'x',
			code: 'record_conflict'
		});
		const onImported = vi.fn();
		const confirmation = new ImportConfirmation(onImported);

		await confirmation.confirm(rows, []);

		expect(onImported).not.toHaveBeenCalled();
		expect(confirmation.importing).toBe(false);
		expect(confirmation.error).toBe('');
		expect(confirmation.notice).toBe(m.import_preview_changed_elsewhere());
		expect(confirmation.reloadKey).toBe(1);

		// もう一度確定したら案内を消す。
		vi.mocked(importRows).mockResolvedValue({ ok: false, message: '取り込めませんでした' });
		await confirmation.confirm(rows, []);
		expect(confirmation.notice).toBe('');
	});

	it('取り込み中に重ねて確定しても、取り込みは1回だけ', async () => {
		const finish = pendingImport();
		const confirmation = new ImportConfirmation(vi.fn());

		const first = confirmation.confirm(rows, []);
		await confirmation.confirm(rows, []);
		finish({ ok: true, deleted: 0, created: 2 });
		await first;

		expect(importRows).toHaveBeenCalledTimes(1);
	});

	it('閉じた後に完了した取り込みの結果は捨てる', async () => {
		const finish = pendingImport();
		const onImported = vi.fn();
		const confirmation = new ImportConfirmation(onImported);

		const pending = confirmation.confirm(rows, []);
		confirmation.reset();
		finish({ ok: true, deleted: 0, created: 2 });
		await pending;

		expect(onImported).not.toHaveBeenCalled();
		expect(confirmation.importing).toBe(false);
	});

	it('想定外の例外でも取り込み中を解除し、例外は投げ直す', async () => {
		vi.mocked(importRows).mockRejectedValue(new Error('boom'));
		const confirmation = new ImportConfirmation(vi.fn());

		await expect(confirmation.confirm(rows, [])).rejects.toThrow('boom');
		expect(confirmation.importing).toBe(false);
	});
});
