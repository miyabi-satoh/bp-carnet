import { describe, expect, it } from 'vitest';
import { render } from 'vitest-browser-svelte';
import { Spinner } from '$lib/components/ui/spinner';

describe('Spinner', () => {
	// 未指定の stroke を undefined で渡すと、@lucide/svelte の既定の線の色が消えて何も描かれない。
	it('stroke を指定しなければ、線の色は既定の currentColor のまま', async () => {
		const screen = await render(Spinner);
		await expect.element(screen.getByRole('status')).toHaveAttribute('stroke', 'currentColor');
	});

	it('stroke を指定すれば、その色で描く', async () => {
		const screen = await render(Spinner, { stroke: 'red' });
		await expect.element(screen.getByRole('status')).toHaveAttribute('stroke', 'red');
	});
});
