import { describe, expect, it } from 'vitest';
import {
	splitMeasuredAt,
	validateBpForm,
	validateRecordForm,
	type RecordFormInput
} from './record-form';
import * as m from '$lib/paraglide/messages.js';

function input(overrides: Partial<RecordFormInput> = {}): RecordFormInput {
	return {
		measuredOnDate: '2026-09-07',
		time: '07:15',
		systolic: '118',
		diastolic: '76',
		pulse: '',
		...overrides
	};
}

describe('validateRecordForm', () => {
	it('returns parsed values when every field is valid (pulse may be empty)', () => {
		expect(validateRecordForm(input())).toEqual({
			ok: true,
			measuredAtLocal: '2026-09-07T07:15',
			systolic: 118,
			diastolic: 76,
			pulse: undefined
		});
	});

	it('明日以降の日付は測定日時の欄のエラーにし、今日なら先の時刻でも通す', () => {
		expect(validateRecordForm(input({ measuredOnDate: '2026-09-08' }), '2026-09-07')).toEqual({
			ok: false,
			errors: { measuredAt: m.record_error_future_date() }
		});
		expect(
			validateRecordForm(input({ measuredOnDate: '2026-09-07', time: '23:59' }), '2026-09-07').ok
		).toBe(true);
	});

	it('reports each empty required field on its own field', () => {
		const result = validateRecordForm(
			input({ measuredOnDate: '', time: '', systolic: ' ', diastolic: '' })
		);
		expect(result).toEqual({
			ok: false,
			errors: {
				measuredAt: m.record_error_required({ field: m.record_measured_at_label() }),
				systolic: m.record_error_required({ field: m.record_systolic_label() }),
				diastolic: m.record_error_required({ field: m.record_diastolic_label() })
			}
		});
	});

	it('treats non-integer text as an error instead of as empty', () => {
		const result = validateRecordForm(input({ pulse: '6八' }));
		expect(result).toEqual({
			ok: false,
			errors: { pulse: m.record_error_not_integer({ field: m.record_pulse_label() }) }
		});
	});

	it('rejects a date or time that does not exist', () => {
		// 写真の読み取り・CSV の取り込みは、端末のピッカーを通らない値を渡してくる。
		// 空文字チェックだけでは通過し、連結した measuredAtLocal が不正なまま確認画面を抜けて、
		// 取り込み全体がサーバーに拒否される (422)。"7:30" は数値としては妥当なので、
		// 実在チェックだけでは素通りする。
		for (const overrides of [
			{ measuredOnDate: '2026-02-30' },
			{ time: '25:99' },
			{ time: '7:30' }
		]) {
			const result = validateRecordForm(input(overrides));
			expect(result.ok).toBe(false);
			if (result.ok) continue;
			expect(result.errors.measuredAt).toBe(
				m.record_error_invalid({ field: m.record_measured_at_label() })
			);
		}
	});

	it('puts the systolic-not-greater error on both blood pressure fields', () => {
		const message = m.record_error_systolic_not_greater();
		expect(validateRecordForm(input({ systolic: '118', diastolic: '120' }))).toEqual({
			ok: false,
			errors: { systolic: message, diastolic: message }
		});
	});

	it('still range-checks a value when the other blood pressure field is empty', () => {
		const result = validateRecordForm(input({ systolic: '', diastolic: '300' }));
		expect(result.ok).toBe(false);
		if (result.ok) return;
		expect(result.errors.diastolic).toBe(m.record_error_diastolic_range({ min: 30, max: 180 }));
	});

	it('reports blood pressure errors even when the measured-at field is also empty', () => {
		const result = validateRecordForm(input({ measuredOnDate: '', pulse: 'x' }));
		expect(result).toEqual({
			ok: false,
			errors: {
				measuredAt: m.record_error_required({ field: m.record_measured_at_label() }),
				pulse: m.record_error_not_integer({ field: m.record_pulse_label() })
			}
		});
	});
});

describe('splitMeasuredAt', () => {
	it('splits a local datetime into the date and time fields', () => {
		expect(splitMeasuredAt('2026-09-07T07:15')).toEqual({
			measuredOnDate: '2026-09-07',
			time: '07:15'
		});
		expect(splitMeasuredAt('')).toEqual({ measuredOnDate: '', time: '' });
	});
});

describe('validateBpForm', () => {
	it('validates only the blood pressure fields (no measured-at field)', () => {
		expect(validateBpForm({ systolic: '128', diastolic: '78', pulse: '68' })).toEqual({
			ok: true,
			systolic: 128,
			diastolic: 78,
			pulse: 68
		});
		expect(validateBpForm({ systolic: '12a', diastolic: '78', pulse: '' })).toEqual({
			ok: false,
			errors: { systolic: m.record_error_not_integer({ field: m.record_systolic_label() }) }
		});
	});
});
