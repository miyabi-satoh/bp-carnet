import { describe, expect, it, vi } from 'vitest';
import { render } from 'vitest-browser-svelte';
import * as m from '$lib/paraglide/messages.js';
import LoadError from './load-error.svelte';

describe('LoadError', () => {
	it('shows only the message when there is nothing to retry', async () => {
		const screen = await render(LoadError, { message: 'failed' });

		await expect.element(screen.getByRole('alert')).toHaveTextContent('failed');
		await expect
			.element(screen.getByRole('button', { name: m.common_retry_button() }))
			.not.toBeInTheDocument();
	});

	it('retries the load and blocks a second press while it runs', async () => {
		let finish: () => void = () => {};
		const onretry = vi.fn(() => new Promise<void>((resolve) => (finish = resolve)));
		const screen = await render(LoadError, { message: 'failed', onretry });
		// 待っている間はスピナーの読み上げが名前に加わるので、名前ではなく役割で探す。
		const button = screen.getByRole('button');

		await screen.getByRole('button', { name: m.common_retry_button() }).click();
		await expect.element(button).toBeDisabled();
		finish();

		await expect.element(button).toBeEnabled();
		expect(onretry).toHaveBeenCalledTimes(1);
	});
});
