import { describe, expect, it } from 'vitest';
import { bpValueIssues } from './bp-values';
import * as m from '$lib/paraglide/messages.js';

describe('bpValueIssues', () => {
	it('targets both fields when systolic is not greater than diastolic', () => {
		expect(bpValueIssues({ systolic: 118, diastolic: 120, pulse: undefined })).toEqual([
			{ fields: ['systolic', 'diastolic'], message: m.record_error_systolic_not_greater() }
		]);
	});

	it('skips the order check while a value is out of range (the range error says enough)', () => {
		const issues = bpValueIssues({ systolic: 50, diastolic: 80, pulse: 10 });
		expect(issues.map((issue) => issue.fields)).toEqual([['systolic'], ['pulse']]);
	});

	it('ignores values that are not entered', () => {
		const issues = bpValueIssues({ systolic: undefined, diastolic: 300, pulse: undefined });
		expect(issues.map((issue) => issue.fields)).toEqual([['diastolic']]);
	});
});
