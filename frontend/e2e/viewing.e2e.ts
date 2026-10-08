// 記録の閲覧 (期間ナビ・平均・グラフ)。
// 平均やグラフは期間内の全記録から決まるため、テストで作った記録だけを持つ使い捨てユーザーで流す。
import type { Page } from '@playwright/test';
import {
	createRecordsByApi,
	fetchTimeZone,
	threeDaysOfMorningAndEvening,
	type NewRecord
} from './api-helpers';
import {
	addDays,
	monthLabel,
	rangeLabel,
	startOfPreviousMonth,
	startOfWeek,
	todayIn,
	weekLabel
} from './date-helpers';
import { userTest as test, expect, NO_LOGIN } from './fixtures';
import { ja } from './helpers';
import { HomePage } from './pages/home-page';

test.use({ storageState: NO_LOGIN });

/**
 * 今日 (ユーザーのタイムゾーン) と、今週・前の週の最初の日。明日以降の日付は登録できない
 * ので、何日分もの記録は前の週に置く。今週の日で今日より前とは限らないため。
 */
async function thisWeek(page: Page) {
	const today = todayIn(await fetchTimeZone(page.request));
	const weekStart = startOfWeek(today);
	return { today, weekStart, previousWeekStart: addDays(weekStart, -7) };
}

test('今週に記録が無ければ、最新の記録を含む週で始まり、「クリア」もその週へ戻る', async ({
	page
}) => {
	const { today, weekStart } = await thisWeek(page);
	const latestDay = addDays(weekStart, -21);
	await createRecordsByApi(page.request, [
		{
			localMeasuredAt: `${addDays(latestDay, -14)}T07:00`,
			systolic: 118,
			diastolic: 78,
			memo: '古い記録'
		},
		{ localMeasuredAt: `${latestDay}T07:00`, systolic: 120, diastolic: 80, memo: '最新の記録' }
	]);

	const home = new HomePage(page);
	await home.open();
	const { thisWeekButton, thisMonthButton, monthButton, filterClearButton } = home.periodNav;

	await expect(thisWeekButton).toHaveText(weekLabel(latestDay));
	await expect(home.recordCard('最新の記録').root).toBeVisible();
	await expect(home.recordCard('古い記録').root).toHaveCount(0);

	// ラベルは、記録の有無によらず実際の今週へ移る。記録はほかの期間にあるので、初めての案内は出さない。
	await thisWeekButton.click();
	await expect(thisWeekButton).toHaveText(weekLabel(today));
	await expect(home.periodEmptyMessage).toBeVisible();
	await expect(home.firstRecordHint).toHaveCount(0);

	// 期間を指定したあとの「クリア」は、最新の記録を含む週へ戻す。
	await home.periodNav.filter(addDays(latestDay, -30), today);
	await filterClearButton.click();
	await expect(thisWeekButton).toHaveText(weekLabel(latestDay));
	await expect(home.recordCard('最新の記録').root).toBeVisible();

	// 月タブは、その最新の記録がある月を出す (週の始まりが前の月でも)。
	await monthButton.click();
	await expect(thisMonthButton).toHaveText(monthLabel(latestDay));
});

test('印刷レポートから戻っても、月タブは最新の記録がある月を出す', async ({ page }) => {
	const { weekStart } = await thisWeek(page);
	const latestDay = addDays(weekStart, -21);
	await createRecordsByApi(page.request, [
		{ localMeasuredAt: `${latestDay}T07:00`, systolic: 120, diastolic: 80, memo: '最新の記録' }
	]);

	const home = new HomePage(page);
	await home.open();
	const report = await home.openReport();
	await report.back();

	await expect(home.periodNav.thisWeekButton).toHaveText(weekLabel(latestDay));
	await home.periodNav.monthButton.click();
	await expect(home.periodNav.thisMonthButton).toHaveText(monthLabel(latestDay));
});

test('記録が1件も無ければ、今週で始まり、下のボタンから記録できると案内する', async ({ page }) => {
	const { today } = await thisWeek(page);
	const home = new HomePage(page);
	await home.open();

	await expect(home.periodNav.thisWeekButton).toHaveText(weekLabel(today));
	await expect(home.firstRecordHint).toContainText(ja.home_add_record_button);
	await expect(home.periodEmptyMessage).toHaveCount(0);
});

test('週・月の切り替えと前後の移動、今週・今月へ戻る、期間の指定', async ({ page }) => {
	const { today, weekStart, previousWeekStart } = await thisWeek(page);
	const previousMonthDay = startOfPreviousMonth(today);
	// 今週は今日の 0:00 に1件 (今より前なので最新の記録になる)、前の週は3日分の朝・夜。
	await createRecordsByApi(page.request, [
		{ localMeasuredAt: `${today}T00:00`, systolic: 118, diastolic: 78, memo: '今週の記録' },
		...threeDaysOfMorningAndEvening(previousWeekStart),
		{
			localMeasuredAt: `${previousMonthDay}T07:00`,
			systolic: 116,
			diastolic: 76,
			memo: '前の月の記録'
		}
	]);

	const home = new HomePage(page);
	await home.open();
	const thisWeekCard = home.recordCard('今週の記録').root;
	const previousWeekCard = home.recordCard('起床後').root;
	const previousMonthCard = home.recordCard('前の月の記録').root;
	const { thisWeekButton, thisMonthButton, prevButton } = home.periodNav;

	// 初期表示は今週。
	await expect(thisWeekButton).toHaveText(weekLabel(today));
	await expect(thisWeekCard).toBeVisible();
	await expect(previousWeekCard).toHaveCount(0);

	await prevButton.click();
	await expect(thisWeekButton).toHaveText(weekLabel(previousWeekStart));
	await expect(previousWeekCard).toBeVisible();
	await expect(thisWeekCard).toHaveCount(0);
	await home.chart.waitForLines(4);

	await thisWeekButton.click();
	await expect(thisWeekButton).toHaveText(weekLabel(today));
	await expect(thisWeekCard).toBeVisible();

	await home.periodNav.monthButton.click();
	await expect(thisMonthButton).toHaveText(monthLabel(today));
	await expect(previousMonthCard).toHaveCount(0);

	await prevButton.click();
	await expect(thisMonthButton).toHaveText(monthLabel(previousMonthDay));
	await expect(previousMonthCard).toBeVisible();

	await thisMonthButton.click();
	await expect(thisMonthButton).toHaveText(monthLabel(today));

	// 開始日が終了日より後なら、記録を取りに行かずに止め、欄の下で知らせる。
	const recordRequests: string[] = [];
	page.on('request', (request) => {
		if (new URL(request.url()).pathname === '/api/v1/records') recordRequests.push(request.url());
	});
	await home.periodNav.filter(weekStart, previousWeekStart);
	await expect(home.periodNav.filterRangeError).toBeVisible();
	await expect(home.periodNav.filterTo).toHaveAttribute('aria-invalid', 'true');
	expect(recordRequests).toEqual([]);

	// 期間を指定すると週/月の選択は外れ、ラベルは指定した期間になる。
	await home.periodNav.applyFilter(previousWeekStart, previousWeekStart);
	await expect(home.periodNav.filterRangeError).toHaveCount(0);
	await expect(
		home.periodNav.rangeButton(rangeLabel(previousWeekStart, previousWeekStart))
	).toBeVisible();
	await expect(previousWeekCard).toBeVisible();
	await expect(previousMonthCard).toHaveCount(0);

	// クリアすると今週に戻る。
	await home.periodNav.filterClearButton.click();
	await expect(thisWeekButton).toHaveText(weekLabel(today));
	await expect(thisWeekCard).toBeVisible();
});

test('2日分以上の記録でグラフが描かれ、系列の表示を切り替えられる', async ({ page }) => {
	// 今週に記録が無いので、画面は記録のある前の週から始まる。
	const { previousWeekStart } = await thisWeek(page);
	const dayRecords = (date: string): NewRecord[] => [
		{ localMeasuredAt: `${date}T07:00`, systolic: 120, diastolic: 80 },
		{ localMeasuredAt: `${date}T21:00`, systolic: 126, diastolic: 84 }
	];
	const home = new HomePage(page);
	const { legend, lines } = home.chart;

	// 1日分だけでは折れ線が引けないので、グラフを出さない。
	await createRecordsByApi(page.request, dayRecords(previousWeekStart));
	await home.open();
	await expect(home.stats.averageCard(ja.bp_stats_morning_average_label)).toBeVisible();
	await expect(legend).toHaveCount(0);

	await createRecordsByApi(page.request, dayRecords(addDays(previousWeekStart, 1)));
	await page.reload();
	await expect(legend).toBeVisible();
	// 初期表示は全4系列。
	await home.chart.waitForLines(4);
	await expect(lines).toHaveCount(4);

	const toggle = (label: string) => home.chart.seriesToggle(label);
	await toggle(ja.chart_morning_systolic_label).click();
	await expect(toggle(ja.chart_morning_systolic_label)).toHaveAttribute('aria-pressed', 'false');
	await expect(lines).toHaveCount(3);

	await toggle(ja.chart_morning_systolic_label).click();
	await expect(toggle(ja.chart_morning_systolic_label)).toHaveAttribute('aria-pressed', 'true');
	await expect(lines).toHaveCount(4);

	// 最後の1系列は非表示にできない。
	await toggle(ja.chart_morning_diastolic_label).click();
	await toggle(ja.chart_evening_systolic_label).click();
	await toggle(ja.chart_evening_diastolic_label).click();
	await expect(lines).toHaveCount(1);
	await expect(toggle(ja.chart_morning_systolic_label)).toBeDisabled();
});

test('朝・夜の平均が、設定の時間帯どおりに分かれる', async ({ page }) => {
	// 今週に記録が無いので、画面は記録のある前の週から始まる。
	const { previousWeekStart: firstDay } = await thisWeek(page);
	const secondDay = addDays(firstDay, 1);
	// 時間帯は既定 (朝 4:00-10:00・夜 18:00-24:00)。同じ日の記録は日別に平均してから平均する。
	// 朝: 1日目 (120+130)/2=125・(80+90)/2=85、2日目 110/70 → 117.5/77.5 → 表示は四捨五入で 118/78。
	// 夜: 140/90 と 150/100 → 145/95。昼の 13:00 はどちらにも入らない。
	await createRecordsByApi(page.request, [
		{ localMeasuredAt: `${firstDay}T07:00`, systolic: 120, diastolic: 80 },
		{ localMeasuredAt: `${firstDay}T09:30`, systolic: 130, diastolic: 90 },
		{ localMeasuredAt: `${firstDay}T13:00`, systolic: 200, diastolic: 110, memo: '昼食後' },
		{ localMeasuredAt: `${firstDay}T20:00`, systolic: 140, diastolic: 90 },
		{ localMeasuredAt: `${secondDay}T06:00`, systolic: 110, diastolic: 70 },
		{ localMeasuredAt: `${secondDay}T21:00`, systolic: 150, diastolic: 100 }
	]);

	const home = new HomePage(page);
	await home.open();
	await expect(home.stats.averageCard(ja.bp_stats_morning_average_label)).toContainText(
		/118\s*\/\s*78/
	);
	await expect(home.stats.averageCard(ja.bp_stats_evening_average_label)).toContainText(
		/145\s*\/\s*95/
	);
	// 朝・夜のどちらにも入らない記録は、一覧には残るが区分が付かない。
	const daytimeCard = home.recordCard('昼食後').root;
	await expect(daytimeCard).toContainText('13:00');
	await expect(daytimeCard).not.toContainText(ja.record_day_period_morning_label);
	await expect(daytimeCard).not.toContainText(ja.record_day_period_evening_label);
});
