import { describe, expect, it } from 'vitest';
import {
	buildChartData,
	buildChartSeries,
	buildChartYDomain,
	DEFAULT_VISIBLE_SERIES_KEYS,
	formatSeriesParam,
	latestPointOf,
	parseSeriesParam,
	toggleSeriesKey,
	visibleChartSeries,
	type ChartPoint
} from './bp-chart';
import type { components } from '$lib/api/schema';

type DailyAverages = components['schemas']['DailyAveragesResponse'];

function day(
	date: string,
	morning: [number, number] | null,
	evening: [number, number] | null
): DailyAverages {
	return {
		date,
		morning: morning ? { systolic: morning[0], diastolic: morning[1] } : null,
		evening: evening ? { systolic: evening[0], diastolic: evening[1] } : null
	};
}

describe('buildChartData', () => {
	it('maps a day with both slots to one point', () => {
		const [point] = buildChartData([day('2026-09-08', [134, 86], [128, 82])]);
		expect(point.morningSystolic).toBe(134);
		expect(point.morningDiastolic).toBe(86);
		expect(point.eveningSystolic).toBe(128);
		expect(point.eveningDiastolic).toBe(82);
	});

	it('leaves the missing slot as null so the line breaks there', () => {
		const [point] = buildChartData([day('2026-09-08', [134, 86], null)]);
		expect(point.eveningSystolic).toBeNull();
		expect(point.eveningDiastolic).toBeNull();
	});

	// `new Date('2026-09-08')` は UTC 解釈になり、JST では前日の 9:00 になってしまう。
	it('parses the date as a local day, not UTC', () => {
		const [point] = buildChartData([day('2026-09-08', [134, 86], null)]);
		expect(point.date.getFullYear()).toBe(2026);
		expect(point.date.getMonth()).toBe(8);
		expect(point.date.getDate()).toBe(8);
	});
});

describe('visibleChartSeries', () => {
	it('keeps the legend order regardless of the order of the given keys', () => {
		const keys = visibleChartSeries(['eveningDiastolic', 'morningSystolic']).map((s) => s.key);
		expect(keys).toEqual(['morningSystolic', 'eveningDiastolic']);
	});
});

describe('toggleSeriesKey', () => {
	it('shows a hidden series', () => {
		expect(toggleSeriesKey(['morningSystolic'], 'eveningSystolic')).toEqual([
			'morningSystolic',
			'eveningSystolic'
		]);
	});

	it('hides a visible series', () => {
		expect(toggleSeriesKey(['morningSystolic', 'eveningSystolic'], 'morningSystolic')).toEqual([
			'eveningSystolic'
		]);
	});

	it('keeps the last visible series', () => {
		expect(toggleSeriesKey(['morningSystolic'], 'morningSystolic')).toEqual(['morningSystolic']);
	});
});

describe('parseSeriesParam', () => {
	it('reads the keys passed from the main page', () => {
		const keys = ['morningSystolic', 'morningDiastolic'] as const;
		expect(parseSeriesParam(formatSeriesParam(keys))).toEqual(keys);
	});

	it('falls back to the default series when the param is missing', () => {
		expect(parseSeriesParam(null)).toEqual(DEFAULT_VISIBLE_SERIES_KEYS);
	});

	it('ignores unknown and duplicated keys', () => {
		expect(parseSeriesParam('eveningSystolic,unknown,eveningSystolic')).toEqual([
			'eveningSystolic'
		]);
	});

	it('falls back to the default series when no key is valid', () => {
		expect(parseSeriesParam('unknown,')).toEqual(DEFAULT_VISIBLE_SERIES_KEYS);
	});
});

describe('buildChartYDomain', () => {
	const days = buildChartData([
		day('2026-09-07', [122, 78], [118, 74]),
		day('2026-09-08', [134, 86], [128, 82])
	]);

	it('covers every value of the given series, rounded outward to tens', () => {
		// 全4系列なら 74〜134 が対象。
		expect(buildChartYDomain(days, buildChartSeries())).toEqual([60, 150]);
	});

	// 上の血圧だけを表示しているときに下の血圧まで含めると、線が上半分に寄る。
	it('ignores series that are not displayed', () => {
		// 上の血圧のみなら 118〜134 が対象。
		const systolicOnly = visibleChartSeries(['morningSystolic', 'eveningSystolic']);
		expect(buildChartYDomain(days, systolicOnly)).toEqual([100, 150]);
	});

	it('returns undefined when no series has a value', () => {
		const empty = buildChartData([day('2026-09-08', null, null)]);
		expect(buildChartYDomain(empty, buildChartSeries())).toBeUndefined();
	});

	it('returns undefined for an empty period', () => {
		expect(buildChartYDomain([], buildChartSeries())).toBeUndefined();
	});
});

describe('latestPointOf', () => {
	const point = (date: string, morning: number | null, evening: number | null): ChartPoint => ({
		date: new Date(date),
		morningSystolic: morning,
		morningDiastolic: null,
		eveningSystolic: evening,
		eveningDiastolic: null
	});

	it('値が入っている最後の点を返す', () => {
		const data = [
			point('2026-09-05', 120, null),
			point('2026-09-06', 130, 128),
			point('2026-09-07', 125, null)
		];
		expect(latestPointOf(data, 'morningSystolic')?.date).toEqual(new Date('2026-09-07'));
		// 末尾が null の系列は、その手前の値が入っている点まで遡る。
		expect(latestPointOf(data, 'eveningSystolic')?.date).toEqual(new Date('2026-09-06'));
	});

	// 単点の系列は線が引けないため、この点に円マークを描けるかどうかが表示の有無を決める。
	it('値が1つだけの系列でもその点を返す', () => {
		const data = [point('2026-09-05', 120, null), point('2026-09-06', 130, 128)];
		expect(latestPointOf(data, 'eveningSystolic')?.date).toEqual(new Date('2026-09-06'));
	});

	it('値が1つも無ければ undefined を返す', () => {
		const data = [point('2026-09-05', 120, null)];
		expect(latestPointOf(data, 'eveningSystolic')).toBeUndefined();
		expect(latestPointOf([], 'morningSystolic')).toBeUndefined();
	});
});
