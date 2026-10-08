// 印刷用レポート。表の行や平均は期間内の全記録から決まるため、使い捨てユーザーで流す。
import { createRecordsByApi, fetchTimeZone, threeDaysOfMorningAndEvening } from './api-helpers';
import { addDays, reportRangeLabel, startOfWeek, todayIn, weekLabel } from './date-helpers';
import { userTest as test, expect, NO_LOGIN } from './fixtures';
import { countPrints, ja } from './helpers';
import { HomePage } from './pages/home-page';

test.use({ storageState: NO_LOGIN });

test('メイン画面の期間と系列を引き継いで開き、ページ内で変えられ、「印刷する」で印刷の画面を開く', async ({
	page
}) => {
	const today = todayIn(await fetchTimeZone(page.request));
	const weekStart = startOfWeek(today);
	const previousWeekStart = addDays(weekStart, -7);
	const previousWeekEnd = addDays(previousWeekStart, 6);
	// 前の週に3日分の朝・夜、今週に1件 (今日の 0:00。今より前なので最新の記録になり、画面は今週から始まる)。
	const previousWeekRecords = threeDaysOfMorningAndEvening(previousWeekStart);
	await createRecordsByApi(page.request, [
		...previousWeekRecords,
		{ localMeasuredAt: `${today}T00:00`, systolic: 118, diastolic: 78, memo: '今週の記録' }
	]);
	const printCalls = await countPrints(page);
	const home = new HomePage(page);
	// ホーム画面とレポートは、同じ部品の期間の切り替えとグラフを持つ。
	const toggle = (label: string) => home.chart.seriesToggle(label);
	const periodButton = home.periodNav.thisWeekButton;

	// メイン画面で前の週を開き、夜・上の血圧を消す。
	await home.open();
	await home.periodNav.prevButton.click();
	await expect(periodButton).toHaveText(weekLabel(previousWeekStart));
	await toggle(ja.chart_evening_systolic_label).click();
	await expect(toggle(ja.chart_evening_systolic_label)).toHaveAttribute('aria-pressed', 'false');

	// 主要な操作なので、タップ領域を44px以上にする (requirements 6.1)。
	expect((await home.reportLink.boundingBox())?.height).toBeGreaterThanOrEqual(44);
	const report = await home.openReport();
	const query = new URL(page.url()).searchParams;
	expect(query.get('from')).toBe(previousWeekStart);
	expect(query.get('to')).toBe(previousWeekEnd);
	expect(query.get('series')).toBe('morningSystolic,morningDiastolic,eveningDiastolic');
	await expect(
		report.rangeLabel(reportRangeLabel(previousWeekStart, previousWeekEnd))
	).toBeVisible();
	await expect(toggle(ja.chart_evening_systolic_label)).toHaveAttribute('aria-pressed', 'false');
	// 表は見出しの行と、期間内の記録の行。
	await expect(report.tableRows).toHaveCount(1 + previousWeekRecords.length);
	await expect(report.cell('今週の記録')).toHaveCount(0);

	// ページ内で次の週へ移り、朝・上の血圧も消す。URL に反映される。
	await report.periodNav.nextButton.click();
	await expect(report.cell('今週の記録')).toBeVisible();
	await expect.poll(() => new URL(page.url()).searchParams.get('from')).toBe(weekStart);
	await report.periodNav.prevButton.click();
	await expect(report.cell('起床後')).toBeVisible();
	await toggle(ja.chart_morning_systolic_label).click();
	await expect
		.poll(() => new URL(page.url()).searchParams.get('series'))
		.toBe('morningDiastolic,eveningDiastolic');
	// 印刷の画面は自動では開かず、「印刷する」で開く。
	expect(await printCalls()).toBe(0);
	await report.printButton.click();
	expect(await printCalls()).toBe(1);

	// 戻ると、メイン画面はレポートを開く前の期間・系列のまま。
	await report.back();
	await expect(periodButton).toHaveText(weekLabel(previousWeekStart));
	await expect(toggle(ja.chart_evening_systolic_label)).toHaveAttribute('aria-pressed', 'false');
	await expect(toggle(ja.chart_morning_systolic_label)).toHaveAttribute('aria-pressed', 'true');
});

test('記録が1件だけなら、平均とグラフを出さず表だけにする', async ({ page }) => {
	const today = todayIn(await fetchTimeZone(page.request));
	// 今より前の時刻に置く (未来の日時は最新の記録に数えない)。
	await createRecordsByApi(page.request, [
		{ localMeasuredAt: `${today}T00:00`, systolic: 118, diastolic: 78, memo: '1件だけ' }
	]);
	const home = new HomePage(page);
	await home.open();
	const report = await home.openReport();

	// 1件だけのときはグラフを出さず、表だけにする。
	await expect(report.cell('1件だけ')).toBeVisible();
	await expect(report.tableRows).toHaveCount(2);
	await expect(report.stats.averageCard(ja.bp_stats_morning_average_label)).toHaveCount(0);
	await expect(report.chart.seriesToggle(ja.chart_morning_systolic_label)).toHaveCount(0);
});
