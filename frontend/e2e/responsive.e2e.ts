// 主要な画面が、スマホの幅 (既定の Pixel 7) と PC の幅の両方で崩れないこと。
// 横スクロールが出ず、主要な操作が見えて押せるかを見る。記録を作るため、ログイン後の画面は使い捨てユーザーで流す。
import type { Page } from '@playwright/test';
import { createRecordsByApi, fetchTimeZone, threeDaysOfMorningAndEvening } from './api-helpers';
import { addDays, startOfWeek, todayIn } from './date-helpers';
import { test, expect, NO_LOGIN } from './fixtures';
import { ja, settledBox } from './helpers';
import { HomePage } from './pages/home-page';
import { LoginPage } from './pages/login-page';
import { SettingsPage } from './pages/settings-page';

test.use({ storageState: NO_LOGIN });

/**
 * ADR: PC の幅は project を足さず、このファイルの中で画面の大きさを切り替えて流す。project にすると
 * すべてのシナリオが2倍流れるため。1280×800 はノート PC でよくある大きさで、`lg` (1024px) を超える。
 */
const LAYOUTS = [
	{ name: 'スマホの幅', wide: false, use: {} },
	{
		name: 'PC の幅',
		wide: true,
		use: {
			viewport: { width: 1280, height: 800 },
			isMobile: false,
			hasTouch: false,
			deviceScaleFactor: 1
		}
	}
];

/** ページが横にスクロールしない。 */
async function expectNoHorizontalScroll(page: Page): Promise<void> {
	await expect
		.poll(() =>
			page.evaluate(
				() => document.documentElement.scrollWidth - document.documentElement.clientWidth
			)
		)
		.toBeLessThanOrEqual(0);
}

for (const { name, wide, use } of LAYOUTS) {
	test.describe(name, () => {
		test.use(use);

		test('ログイン画面', async ({ page }) => {
			const login = new LoginPage(page);
			await login.open();
			await expect(login.username).toBeVisible();
			await expect(login.submitButton).toBeInViewport();
			await expectNoHorizontalScroll(page);
		});

		test('メイン画面・記録フォーム・印刷用レポート・設定', async ({
			page,
			throwawayUser: user
		}) => {
			// 明日以降の日付は登録できないので、前の週に置く。画面はその週から始まる。
			const weekStart = startOfWeek(todayIn(await fetchTimeZone(page.request)));
			await createRecordsByApi(page.request, threeDaysOfMorningAndEvening(addDays(weekStart, -7)));

			const home = new HomePage(page);
			await home.open();
			await home.chart.waitForLines(4);
			await expect(home.header.userMenuButton).toBeVisible();
			await expect(home.periodNav.prevButton).toBeVisible();
			await expectNoHorizontalScroll(page);

			// 記録の追加は画面下端に固定したバーから。
			await expect(home.addRecordButton).toBeInViewport();
			const sheet = await home.openAddSheet();
			// 記録フォームは、スマホでは下端からのシート、`sm` 以上では中央のダイアログ。
			await expect(sheet.createButton).toBeInViewport();
			const box = await settledBox(sheet.root);
			const viewportHeight = page.viewportSize()!.height;
			if (wide) {
				expect(Math.abs(box.y + box.height / 2 - viewportHeight / 2)).toBeLessThanOrEqual(1);
			} else {
				expect(Math.abs(box.y + box.height - viewportHeight)).toBeLessThanOrEqual(1);
			}
			await page.keyboard.press('Escape');
			await expect(sheet.root).toBeHidden();

			const report = await home.openReport();
			await report.chart.waitForLines(4);
			await expect(report.printButton).toBeVisible();
			await expectNoHorizontalScroll(page);
			// 広い幅では紙面と同じく、平均 (左) とグラフ (右) を横に並べ、
			// 表は上・下の血圧を別の列にする。スマホ幅では縦に積み、1列にまとめる。
			const averageBox = await settledBox(
				report.stats.averageCard(ja.bp_stats_morning_average_label)
			);
			const legendBox = await settledBox(report.chart.legend);
			const systolicHeader = report.columnHeader(ja.record_systolic_label);
			const pairHeader = report.columnHeader(ja.record_bp_pair_label);
			if (wide) {
				expect(legendBox.x).toBeGreaterThanOrEqual(averageBox.x + averageBox.width);
				await expect(systolicHeader).toBeVisible();
				await expect(pairHeader).toBeHidden();
			} else {
				expect(legendBox.y).toBeGreaterThanOrEqual(averageBox.y + averageBox.height);
				await expect(systolicHeader).toBeHidden();
				await expect(pairHeader).toBeVisible();
			}

			const settings = new SettingsPage(page);
			await settings.open();
			await expect(settings.username(user.username)).toBeVisible();
			await expect(settings.timeZone).toBeVisible();
			await expect(settings.exportCsvButton).toBeEnabled();
			await expectNoHorizontalScroll(page);
		});
	});
}
