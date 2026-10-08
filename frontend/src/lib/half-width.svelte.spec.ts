import { describe, expect, it } from 'vitest';
import { page, userEvent } from 'vitest/browser';
import { render } from 'vitest-browser-svelte';
import Harness from './half-width.test-harness.svelte';

describe('halfWidthInput', () => {
	it('全角で入れた値を、欄と bind した値の両方で半角にする', async () => {
		render(Harness);
		const input = page.getByRole('textbox', { name: '値' });

		await userEvent.fill(input, '１２０');

		await expect.element(input).toHaveValue('120');
		await expect.element(page.getByTestId('value')).toHaveTextContent('120');
	});

	it('変換中 (未確定) の間は書き換えない', async () => {
		render(Harness);
		const input = page.getByRole('textbox', { name: '値' });
		const element = input.element() as HTMLInputElement;

		element.value = '１２';
		element.dispatchEvent(new InputEvent('input', { bubbles: true, isComposing: true }));
		expect(element.value).toBe('１２');

		element.dispatchEvent(new CompositionEvent('compositionend', { bubbles: true }));
		await expect.element(input).toHaveValue('12');
		await expect.element(page.getByTestId('value')).toHaveTextContent('12');
	});
});
