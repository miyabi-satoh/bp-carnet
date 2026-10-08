import { describe, expect, it } from 'vitest';
import * as m from '$lib/paraglide/messages.js';
import {
	DEFAULT_PERIOD_THRESHOLDS as defaults,
	dayPeriodAt,
	autoTimezoneLabel,
	periodThresholdErrors,
	timezoneChoiceOptions,
	timezoneLabel
} from '$lib/settings';

describe('dayPeriodAt', () => {
	it('朝・夜とも [開始, 終了) として振り分ける', () => {
		expect(dayPeriodAt('04:00', defaults)).toBe('morning');
		expect(dayPeriodAt('09:59', defaults)).toBe('morning');
		expect(dayPeriodAt('10:00', defaults)).toBeUndefined();
		expect(dayPeriodAt('18:00', defaults)).toBe('evening');
		expect(dayPeriodAt('23:59', defaults)).toBe('evening');
	});

	it('HH:MM として解釈できない・実在しない時刻はどちらにも振り分けない', () => {
		expect(dayPeriodAt('7:30', defaults)).toBeUndefined();
		expect(dayPeriodAt('', defaults)).toBeUndefined();
		expect(dayPeriodAt('07:60', defaults)).toBeUndefined();
		expect(dayPeriodAt('24:00', defaults)).toBeUndefined();
	});
});

describe('periodThresholdErrors', () => {
	it('既定の時間帯はエラーにしない', () => {
		expect(periodThresholdErrors(defaults, 'morning')).toEqual({});
	});

	it('朝の終了と夜の開始が接するだけなら重なりとしない', () => {
		expect(periodThresholdErrors({ ...defaults, morningEndMin: 1080 }, 'morning')).toEqual({});
	});

	it('開始が終了以降の行にエラーを出す', () => {
		expect(periodThresholdErrors({ ...defaults, morningStartMin: 600 }, 'evening')).toEqual({
			morning: m.settings_period_not_ascending_error()
		});
		expect(periodThresholdErrors({ ...defaults, eveningStartMin: 1440 }, 'morning')).toEqual({
			evening: m.settings_period_not_ascending_error()
		});
	});

	it('朝・夜の重なりは最後に変えた側の行に出す', () => {
		const overlapping = { ...defaults, eveningStartMin: 540 };
		expect(periodThresholdErrors(overlapping, 'evening')).toEqual({
			evening: m.settings_period_overlapping_error()
		});
		expect(periodThresholdErrors(overlapping, 'morning')).toEqual({
			morning: m.settings_period_overlapping_error()
		});
	});

	it('開始と終了の前後が誤っていれば、重なりは出さない', () => {
		expect(
			periodThresholdErrors({ ...defaults, morningStartMin: 1200, morningEndMin: 1200 }, 'morning')
		).toEqual({ morning: m.settings_period_not_ascending_error() });
	});
});

describe('timezoneLabel', () => {
	it('IANA 名に表示名を添える', () => {
		expect(timezoneLabel('Asia/Tokyo')).toMatch(/^Asia\/Tokyo \(.+\)$/);
	});

	it('閲覧環境が知らないゾーンは IANA 名だけにする', () => {
		expect(timezoneLabel('Not/A_Zone')).toBe('Not/A_Zone');
	});
});

describe('timezoneChoiceOptions', () => {
	// 夏時間の無い1月で時差を固定する。
	const january = new Date('2026-01-15T00:00:00Z');

	it('今の時差の順に「(UTC+09:00) 表示名」の形で並べる', () => {
		const options = timezoneChoiceOptions(['Asia/Tokyo', 'Etc/UTC', 'America/New_York'], january);
		expect(options.map((option) => option.zone)).toEqual([
			'America/New_York',
			'Etc/UTC',
			'Asia/Tokyo'
		]);
		expect(options[0].label).toMatch(/^\(UTC-05:00\) /);
		expect(options[1].label).toMatch(/^\(UTC\+00:00\) /);
		expect(options[2].label).toMatch(/^\(UTC\+09:00\) /);
	});

	it('表示名を持たないゾーンは IANA 名で出す', () => {
		const [utc] = timezoneChoiceOptions(['Etc/UTC'], january);
		expect(utc.label).toBe('(UTC+00:00) Etc/UTC');
	});

	it('同じ表示名が並ぶゾーンには都市名を添える', () => {
		const options = timezoneChoiceOptions(['Europe/Paris', 'Europe/Berlin'], january);
		expect(
			options.map((option) => option.label.endsWith(`(${option.zone.split('/')[1]})`))
		).toEqual([true, true]);
	});
});

describe('autoTimezoneLabel', () => {
	it('決まったタイムゾーンの IANA 名と表示名を添える', () => {
		expect(autoTimezoneLabel('Asia/Tokyo')).toMatch(
			new RegExp(
				`^${m.settings_timezone_auto_option({ zone: 'Asia/Tokyo .+' }).replace(/[()]/g, '\\$&')}$`
			)
		);
	});
});
