import { describe, expect, it } from 'vitest';
import { render } from 'vitest-browser-svelte';
import * as m from '$lib/paraglide/messages.js';
import Harness from './records-overview.test-harness.svelte';

describe('RecordsOverview', () => {
	it('記録が多いときは新しい順に100件だけ出し、「さらに表示」で100件ずつ足す', async () => {
		const screen = await render(Harness, { count: 250 });

		await expect.element(screen.getByText('記録100', { exact: true })).toBeInTheDocument();
		await expect.element(screen.getByText('記録101', { exact: true })).not.toBeInTheDocument();

		await screen
			.getByRole('button', { name: m.record_list_show_more_button({ count: 100 }) })
			.click();
		await expect.element(screen.getByText('記録200', { exact: true })).toBeInTheDocument();
		await expect.element(screen.getByText('記録201', { exact: true })).not.toBeInTheDocument();

		// 残りが100件に満たなければ、残りの件数を出す。
		await screen
			.getByRole('button', { name: m.record_list_show_more_button({ count: 50 }) })
			.click();
		await expect.element(screen.getByText('記録250', { exact: true })).toBeInTheDocument();
		await expect.element(screen.getByRole('button', { name: /さらに/ })).not.toBeInTheDocument();
	});

	it('100件以下なら「さらに表示」を出さない', async () => {
		const screen = await render(Harness, { count: 100 });

		await expect.element(screen.getByText('記録100', { exact: true })).toBeInTheDocument();
		await expect.element(screen.getByRole('button', { name: /さらに/ })).not.toBeInTheDocument();
	});
});
