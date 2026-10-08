import { beforeEach, describe, expect, it, vi } from 'vitest';
import { NewPasswordState } from './new-password.svelte';
import type { PasswordPolicy } from '$lib/password';

vi.mock('$lib/password', async (importOriginal) => ({
	...(await importOriginal<typeof import('$lib/password')>()),
	fetchPasswordPolicy: vi.fn()
}));
import { fetchPasswordPolicy } from '$lib/password';

const fetchMock = vi.mocked(fetchPasswordPolicy);

const policy: PasswordPolicy = { minLength: 4, requiredClasses: [] };

beforeEach(() => {
	fetchMock.mockReset();
});

describe('NewPasswordState.loadPolicy', () => {
	it('fetches the policy only once', async () => {
		fetchMock.mockResolvedValue({ ok: true, data: policy });
		const state = new NewPasswordState();

		await state.loadPolicy();
		await state.loadPolicy();

		expect(state.policy).toEqual(policy);
		expect(fetchMock).toHaveBeenCalledTimes(1);
	});

	it('keeps the error message on failure and clears it on retry', async () => {
		fetchMock.mockResolvedValueOnce({ ok: false, message: 'failed' });
		const state = new NewPasswordState();

		await state.loadPolicy();
		expect(state.policy).toBeNull();
		expect(state.loadErrorMessage).toBe('failed');

		fetchMock.mockResolvedValueOnce({ ok: true, data: policy });
		await state.loadPolicy();
		expect(state.policy).toEqual(policy);
		expect(state.loadErrorMessage).toBe('');
	});
});

describe('NewPasswordState.validate', () => {
	it('rejects without marking submitted while the policy is not loaded', () => {
		const state = new NewPasswordState();
		state.password = 'pass';
		state.confirmation = 'pass';

		expect(state.validate()).toBe(false);
		expect(state.submitted).toBe(false);
	});

	it('marks submitted and checks both fields against the policy', async () => {
		fetchMock.mockResolvedValue({ ok: true, data: policy });
		const state = new NewPasswordState();
		await state.loadPolicy();

		state.password = 'pass';
		state.confirmation = 'other';
		expect(state.validate()).toBe(false);
		expect(state.submitted).toBe(true);

		state.confirmation = 'pass';
		expect(state.validate()).toBe(true);
	});

	it('reset clears the inputs and the submitted flag', async () => {
		fetchMock.mockResolvedValue({ ok: true, data: policy });
		const state = new NewPasswordState();
		await state.loadPolicy();
		state.password = 'pass';
		state.confirmation = 'pass';
		state.validate();

		state.reset();

		expect(state.password).toBe('');
		expect(state.confirmation).toBe('');
		expect(state.submitted).toBe(false);
		expect(state.policy).toEqual(policy);
	});
});
