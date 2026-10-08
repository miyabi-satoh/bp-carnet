import { describe, expect, it } from 'vitest';
import { render } from 'vitest-browser-svelte';
import * as m from '$lib/paraglide/messages.js';
import Harness from './import-rows.test-harness.svelte';

describe('ImportRows', () => {
	// 見出しの id は部品の中で作るので、欄の名前として読み上げられるかを確かめる。
	it('一覧の欄は、見出しを名前に持つ', async () => {
		const screen = await render(Harness);
		await expect
			.element(screen.getByRole('region', { name: m.import_rows_title() }))
			.toBeInTheDocument();
	});

	// チェックの通知は親の行を読むため、書き戻しより先に呼ばれると古い値を見てしまう。
	it('onkeepchange を呼ぶ時点で、チェックが親の行に書き戻されている', async () => {
		const screen = await render(Harness);
		const keeps = screen.getByLabelText(m.record_row_keep_label());

		await keeps.first().click();
		await expect.element(screen.getByTestId('seen-on-keep-change')).toHaveTextContent('000');

		await keeps.nth(2).click();
		await expect.element(screen.getByTestId('seen-on-keep-change')).toHaveTextContent('001');
	});

	// チェックしても取り込まれないため、直すまでチェックさせない (docs/import-export.md)。
	it('エラーのある行は、チェックできない', async () => {
		const screen = await render(Harness);
		const keep = screen.getByLabelText(m.record_row_keep_label()).nth(1);
		await expect.element(keep).toBeDisabled();
		await expect.element(keep).not.toBeChecked();
	});

	// 値を確かめようと触れただけで、取り込む記録から外れないように。
	it('行の文字を押しても、チェックは切り替わらない', async () => {
		const screen = await render(Harness);
		const keep = screen.getByLabelText(m.record_row_keep_label()).first();
		await expect.element(keep).toBeChecked();

		await screen.getByRole('listitem').first().getByText(/120/).first().click();

		await expect.element(keep).toBeChecked();
		await expect.element(screen.getByTestId('seen-on-keep-change')).toHaveTextContent('');
	});

	it('「直す」は、その行を渡す', async () => {
		const screen = await render(Harness);
		await screen.getByRole('button', { name: /21:00/ }).click();
		await expect.element(screen.getByTestId('edited-time')).toHaveTextContent('21:00');
	});

	// 行が多いときに、確かめてほしい行だけを見られるようにする (docs/import-export.md)。
	it('「要確認だけ」でエラーのある行だけに絞る', async () => {
		const screen = await render(Harness);
		expect(screen.getByRole('listitem').elements()).toHaveLength(3);

		await screen.getByRole('button', { name: m.import_rows_needs_review_filter() }).click();
		const rows = screen.getByRole('listitem').elements();
		expect(rows).toHaveLength(1);
		expect(rows[0].textContent).toContain('300');
	});
});
