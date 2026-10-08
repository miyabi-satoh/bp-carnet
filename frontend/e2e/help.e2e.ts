// 使い方のページ (`/help`)。ログインなしで読めること、目次・前後の項目・入口を辿れること。
// 後半は各項目の手順を画面で辿るシナリオで、`just help-shots` のときは1歩ごとに手順の画像を撮る (→ help-shots.ts)。
// 手順どおりに操作できなくなったら、画像を撮り直す合図にもなる。
import { readFile } from 'node:fs/promises';
import type { Page } from '@playwright/test';
import {
	createRecordByApi,
	createRecordsByApi,
	fetchTimeZone,
	resetSettingsByApi,
	threeDaysOfMorningAndEvening,
	type NewRecord
} from './api-helpers';
import { mockAuthProviders, PRODUCTION_AUTH_PROVIDERS } from './auth-mocks';
import { addDays, hourIn, startOfWeek, todayIn } from './date-helpers';
import { test, userTest, expect, NO_LOGIN } from './fixtures';
import { clearLogin, countPrints, ja, waitForApiResponse } from './helpers';
import { HelpIndexPage } from './pages/help-index-page';
import { HelpTopicPage } from './pages/help-topic-page';
import { HomePage } from './pages/home-page';
import { LoginPage } from './pages/login-page';
import { SettingsPage } from './pages/settings-page';
import { TopupResultPage } from './pages/topup-result-page';
import { VerifyEmailPage } from './pages/verify-email-page';
import { HELP_SHOTS_ENABLED, SAMPLE_TEXT, takeHelpShot } from './help-shots';
import {
	errorResponse,
	freeQuota,
	mockOcrStatus,
	mockOcrExtract,
	monitorResult,
	notebookResult,
	noValuesResult,
	samplePhoto,
	type NotebookRow
} from './ocr-mocks';

test('画面を移ると、開いているページに合わせて title が変わる', async ({ page }) => {
	const home = new HomePage(page);
	await page.goto('/');
	await expect(page).toHaveTitle(ja.seo_top_title);
	await home.header.chooseUserMenuItem(ja.help_title);
	await expect(page).toHaveTitle(`${ja.help_title} — ${ja.app_name}`);
	await new HelpIndexPage(page).openTopic(ja.help_login_title, 'login');
	await expect(page).toHaveTitle(`${ja.help_login_title} — ${ja.help_title} — ${ja.app_name}`);
	// ログインが要る画面に移ると、公開ページの title は残らない。
	await page.goBack();
	await page.goBack();
	await expect(page).toHaveTitle(ja.seo_top_title);
	await home.header.chooseUserMenuItem(ja.settings_title);
	await page.waitForURL('/settings');
	await expect(page).toHaveTitle(ja.app_name);
});

test.describe('ログインなし', () => {
	test.use({ storageState: NO_LOGIN });

	test('公開ページの title はページ別になる', async ({ page }) => {
		await page.goto('/terms');
		await expect(page).toHaveTitle(`${ja.terms_title} — ${ja.app_name}`);
		await page.goto('/privacy');
		await expect(page).toHaveTitle(`${ja.privacy_title} — ${ja.app_name}`);
		await page.goto('/help/login');
		await expect(page).toHaveTitle(`${ja.help_login_title} — ${ja.help_title} — ${ja.app_name}`);
	});

	test('目次から項目へ進み、前後の項目・目次に戻る・戻るボタンを辿れる', async ({ page }) => {
		const index = new HelpIndexPage(page);
		await index.open();
		await expect(index.heading).toBeVisible();

		const topic = await index.openTopic(ja.help_login_title, 'login');
		const { prevLink, nextLink } = topic;
		await expect(topic.heading(ja.help_login_title)).toBeVisible();
		await expect(prevLink).toHaveCount(0);
		// 手順の画像が同梱されている。撮り直しの実行では、まだビルドに入っていないので見ない。
		if (!HELP_SHOTS_ENABLED) {
			const images = topic.stepImages;
			await expect(images).toHaveCount(3);
			for (const image of await images.all()) {
				await image.scrollIntoViewIfNeeded();
				await expect(image).toHaveJSProperty('complete', true);
				expect(await image.evaluate((img: HTMLImageElement) => img.naturalWidth)).toBeGreaterThan(
					0
				);
			}
		}

		await nextLink.click();
		await page.waitForURL('/help/signup');
		await expect(topic.heading(ja.help_signup_title)).toBeVisible();

		await prevLink.click();
		await page.waitForURL('/help/login');

		await topic.backToIndexLink.click();
		await page.waitForURL('/help');

		const lastTopic = await index.openTopic(
			ja.help_home_screen_android_title,
			'home-screen-android'
		);
		await expect(lastTopic.nextLink).toHaveCount(0);
		await lastTopic.backLink.click();
		await page.waitForURL('/help');
	});

	test('ログイン画面から使い方を開ける', async ({ page }) => {
		const login = new LoginPage(page);
		await login.open();
		await login.helpLink.click();
		await page.waitForURL('/help');
		await expect(new HelpIndexPage(page).heading).toBeVisible();
	});

	test('ホーム画面に追加する項目は、画像の代わりに公式の手順を新しいタブで開くリンクを出す', async ({
		page
	}) => {
		const topic = new HelpTopicPage(page);
		// 外のサイトには行かず、リンク先と開き方だけを確かめる。リンクの名前は本文 ($lib/help/ja/*.md) のもの。
		const guides = [
			[
				'home-screen',
				ja.help_home_screen_title,
				'詳しい手順 (Apple のサイト)',
				'https://support.apple.com/ja-jp/guide/iphone/iphea86e5236/ios'
			],
			[
				'home-screen-android',
				ja.help_home_screen_android_title,
				'詳しい手順 (Google のサイト)',
				'https://support.google.com/chrome/answer/9658361?hl=ja&co=GENIE.Platform%3DAndroid'
			]
		] as const;
		for (const [slug, title, label, href] of guides) {
			await topic.open(slug);
			await expect(topic.heading(title)).toBeVisible();
			await expect(topic.steps).toHaveCount(3);
			await expect(topic.stepImages).toHaveCount(0);
			await expect(topic.stepLinks).toHaveCount(1);
			const link = topic.externalLink(label);
			await expect(link).toHaveAttribute('href', href);
			await expect(link).toHaveAttribute('target', '_blank');
			await expect(link).toHaveAttribute('rel', /\bnoopener\b/);
		}
	});

	test('無い項目は開けない', async ({ page }) => {
		const topic = new HelpTopicPage(page);
		await topic.open('no-such-topic');
		await expect(topic.heading('404')).toBeVisible();
	});
});

test('ユーザーメニューから使い方を開ける', async ({ page }) => {
	const home = new HomePage(page);
	await home.open();
	await home.header.chooseUserMenuItem(ja.help_title);
	await page.waitForURL('/help');
	await expect(new HelpIndexPage(page).heading).toBeVisible();
});

/**
 * いつ撮っても今より前になる、いちばん近い朝 (7:00) の日。朝・夜の平均の注意書きが出ないよう、朝の時間帯に入れる。
 * 7時より前に撮るときは前日にする。
 */
function latestPastMorningDay(timeZone: string): string {
	const today = todayIn(timeZone);
	return hourIn(timeZone) >= 7 ? today : addDays(today, -1);
}

/** 開いたままの画面で記録を作った後、`day` が前の週 (日曜の7時より前に撮った前日) なら、表示中の
 * 今週に入らないので、その週へ移る。開き直す画面は、最新の記録を含む週から始まる。 */
async function showWeekOf(page: Page, day: string, today: string): Promise<void> {
	if (day < startOfWeek(today)) {
		await new HomePage(page).periodNav.prevButton.click();
	}
}

/** 1週間分の朝 (7:00)・夜 (21:00) の見本。日ごとに値を揺らし、グラフで変わり方が見えるようにする。
 * 最初の記録のメモは「起床後」にする。 */
const WEEK_SAMPLE = [
	{ morning: [128, 84, 68], evening: [122, 78, 66] },
	{ morning: [132, 86, 70], evening: [125, 80, 67] },
	{ morning: [126, 82, 67], evening: [120, 76, 65] },
	{ morning: [135, 88, 71], evening: [128, 82, 68] },
	{ morning: [130, 85, 69], evening: [124, 79, 66] },
	{ morning: [127, 83, 68], evening: [121, 77, 65] },
	{ morning: [133, 86, 70], evening: [126, 81, 67] }
] as const;

function weekOfMorningAndEvening(firstDay: string): NewRecord[] {
	const records = WEEK_SAMPLE.flatMap(({ morning, evening }, offset): NewRecord[] =>
		(
			[
				['07:00', morning],
				['21:00', evening]
			] as const
		).map(([time, [systolic, diastolic, pulse]]) => ({
			localMeasuredAt: `${addDays(firstDay, offset)}T${time}`,
			systolic,
			diastolic,
			pulse
		}))
	);
	records[0].memo = '起床後';
	return records;
}

/**
 * 先週の7日分の朝・夜を作り、その週のメイン画面をグラフが描けるまで開く (画面は最新の記録を含む週から始まる)。
 * 先週にするのは、撮った日によってグラフに先の日の記録が写らないようにするため。
 */
async function openWeekWithPastRecords(page: Page): Promise<HomePage> {
	const today = todayIn(await fetchTimeZone(page.request));
	await createRecordsByApi(page.request, weekOfMorningAndEvening(addDays(startOfWeek(today), -7)));
	const home = new HomePage(page);
	await home.open();
	await home.chart.waitForLines(4);
	return home;
}

/**
 * ログインの方法とパスワードの決まりの問い合わせに、本番と同じ構成 (LINE・Google・Apple・8文字以上、
 * deploy/cloud/config.production.toml) で答える。
 * 使い方の画面は本番の構成で撮る。
 * オープンβの表示は、終われば画像が古くなるので出さない。
 */
async function mockProductionProviders(page: Page): Promise<void> {
	await mockAuthProviders(page, { ...PRODUCTION_AUTH_PROVIDERS, appleEnabled: true });
	await page.route('**/api/v1/auth/password-policy', (route) =>
		route.fulfill({ json: { minLength: 8, requiredClasses: [] } })
	);
}

/** ユーザーメニューから設定を開き、読み込みを待つ。1歩目 (`shotName`) はメニューを撮る。 */
async function openSettings(page: Page, shotName: string): Promise<SettingsPage> {
	const home = new HomePage(page);
	await home.open();
	const { header } = home;
	await header.userMenuButton.click();
	const menuItem = header.menuItem(ja.settings_title);
	await takeHelpShot(page, shotName, [menuItem]);
	await menuItem.click();
	await page.waitForURL('/settings');
	const settings = new SettingsPage(page);
	await expect(settings.timeZone).toBeVisible();
	return settings;
}

/**
 * フォーカスを外す。閉じたダイアログから戻ったフォーカスの枠を、押す場所と紛れないよう写さない。
 */
async function blurFocus(page: Page): Promise<void> {
	await page.evaluate(() => (document.activeElement as HTMLElement | null)?.blur());
}

userTest.describe('手順', () => {
	userTest.use({ storageState: NO_LOGIN });
	userTest.beforeEach(async ({ page }) => {
		await mockProductionProviders(page);
		// 本番は写真の読み取りが有効で、ホームに「写真から」のボタンが常に出る。
		// 無料の枠を使い始める前 (残り100%) の、買い足せる構成で撮る。
		await mockOcrStatus(page, { quotas: [freeQuota(100)], topupAvailable: true });
	});

	userTest('ログインする', async ({ page, throwawayUser: user }) => {
		// 使い捨てユーザーは作った時点でログイン済みなので、ログアウトした状態に戻す。
		await clearLogin(page);
		const login = new LoginPage(page);
		await login.open();
		const { username, password, submitButton: submit } = login;
		// e2e の backend は LINE・Google・Apple でログインできないので、ボタンは撮るだけにする。
		await takeHelpShot(page, 'login-1', [login.lineButton, login.googleButton, login.appleButton]);

		// 画像には使い捨てユーザーの ID を写さないよう、見本の値で撮ってから入れ替えて送る。
		await username.fill(SAMPLE_TEXT);
		await password.fill('password');
		await takeHelpShot(page, 'login-2', [username, password, submit]);

		await username.fill(user.username);
		await password.fill(user.password);
		const home = await login.submit();
		await expect(home.heading).toBeVisible();
		await takeHelpShot(page, 'login-3');
	});

	userTest('メールアドレスでアカウントを作る', async ({ page }) => {
		await clearLogin(page);
		const login = new LoginPage(page);
		await login.open();
		await takeHelpShot(page, 'signup-1', [login.signupLink]);

		// メールは送らず、画面だけを辿る (届いたメールのリンクまで通すのは mail.e2e.ts)。
		const signup = await login.openSignup();
		await signup.email.fill(SAMPLE_TEXT);
		await expect(signup.submitButton).toBeEnabled();
		await takeHelpShot(page, 'signup-2', [signup.email, signup.submitButton]);

		await page.route('**/api/v1/auth/signup/check', (route) => route.fulfill({ status: 204 }));
		await page.goto('/verify-email?token=sample');
		const verify = new VerifyEmailPage(page);
		await verify.password.fill('password1234');
		await verify.passwordConfirmation.fill('password1234');
		await takeHelpShot(page, 'signup-3', [
			verify.password,
			verify.passwordConfirmation,
			verify.submitButton
		]);
	});

	userTest('手動で入力する', async ({ page }) => {
		const home = new HomePage(page);
		await home.open();
		await takeHelpShot(page, 'record-by-hand-1', [home.addRecordButton]);

		const sheet = await home.openAddSheet();
		const { systolic, diastolic, pulse } = sheet.fields;
		// 日時は既定の現在時刻だと、撮る時刻によって朝・夜のどちらにも入らず、平均の代わりに注意書きが出る。
		const timeZone = await fetchTimeZone(page.request);
		const day = latestPastMorningDay(timeZone);
		await sheet.fields.date.fill(day);
		await sheet.fields.time.fill('07:00');
		await sheet.fields.fill({ systolic: '120', diastolic: '80', pulse: '70' });
		await takeHelpShot(page, 'record-by-hand-2', [systolic, diastolic, pulse]);

		await takeHelpShot(page, 'record-by-hand-3', [sheet.createButton]);
		const created = waitForApiResponse(page, 'POST', '/records');
		await sheet.createButton.click();
		expect((await created).status()).toBe(201);
		await expect(sheet.root).toBeHidden();
		await showWeekOf(page, day, todayIn(timeZone));

		const card = home.recordCard(ja.record_pulse_with_value.replace('{pulse}', '70')).root;
		await expect(card).toContainText(/120\s*\/\s*80/);
		await takeHelpShot(page, 'record-by-hand-4', [card]);
	});

	userTest('写真で記録する', async ({ page }) => {
		const values = { systolic: 120, diastolic: 80, pulse: 70 };
		await mockOcrExtract(page, { json: monitorResult(values) });
		const home = new HomePage(page);
		await home.open();
		await takeHelpShot(page, 'record-by-photo-1', [home.photoButton]);

		const photoPage = await home.choosePhoto(await samplePhoto(page, { kind: 'monitor', values }));
		await expect(photoPage.photoPreview.image).toHaveJSProperty('complete', true);
		await takeHelpShot(page, 'record-by-photo-2', [photoPage.readButton]);
		await photoPage.readButton.click();
		const { fields } = photoPage;
		await expect(fields.systolic).toHaveValue('120');
		await takeHelpShot(page, 'record-by-photo-3', [
			fields.systolic,
			fields.diastolic,
			fields.pulse
		]);

		const { date: measuredAt, time: measuredTime } = fields;
		// 既定の現在時刻のままだと、撮るたびに画像が変わる。
		const day = latestPastMorningDay(await fetchTimeZone(page.request));
		await measuredAt.fill(day);
		await measuredTime.fill('07:00');
		const submit = photoPage.createButton;
		await blurFocus(page);
		await takeHelpShot(page, 'record-by-photo-4', [measuredAt, measuredTime, submit]);
		const created = waitForApiResponse(page, 'POST', '/records');
		await submit.click();
		expect((await created).status()).toBe(201);
		await page.waitForURL('/');
	});

	userTest('手帳の記録をまとめて取り込む', async ({ page }) => {
		// 読み取った行は、いつ撮っても先の日時にならないよう、一昨日と昨日の朝・夜にする。
		const today = todayIn(await fetchTimeZone(page.request));
		const rows = [-2, -1].flatMap((offset, i): NotebookRow[] => [
			{
				date: addDays(today, offset),
				time: '07:00',
				systolic: 120 + i * 2,
				diastolic: 80,
				pulse: 70
			},
			{
				date: addDays(today, offset),
				time: '21:00',
				systolic: 124 - i * 2,
				diastolic: 82,
				pulse: 68
			}
		]);
		await mockOcrExtract(page, { json: notebookResult(rows) });
		const home = new HomePage(page);
		await home.open();
		await takeHelpShot(page, 'import-notebook-1', [home.photoButton]);
		const photoPage = await home.choosePhoto(await samplePhoto(page, { kind: 'notebook', rows }));
		await expect(photoPage.photoPreview.image).toHaveJSProperty('complete', true);
		await takeHelpShot(page, 'import-notebook-2', [photoPage.readButton]);
		await photoPage.readButton.click();
		const { rows: importRows, preview, reviewButton: review } = photoPage;

		await expect(importRows.items).toHaveCount(rows.length);
		// 置き換えられる記録からのメモの引き継ぎが終わるまでは押せない。
		await expect(review).toBeEnabled();
		// 取り込む行を選ぶチェックと、値を直す入口を囲む。
		const firstRow = importRows.items.first();
		await takeHelpShot(page, 'import-notebook-3', [
			importRows.keepCheckbox(firstRow),
			importRows.editButton(firstRow)
		]);
		await takeHelpShot(page, 'import-notebook-4', [review]);

		await review.click();
		await expect(preview.heading).toBeVisible();
		await expect(page.getByText(ja.import_preview_status_added, { exact: true })).toHaveCount(
			rows.length
		);
		const confirm = preview.confirmButton;
		await expect(confirm).toBeEnabled();
		await takeHelpShot(page, 'import-notebook-5', [confirm]);
		const imported = waitForApiResponse(page, 'POST', '/records/import');
		await confirm.click();
		expect((await imported).ok()).toBe(true);
		await page.waitForURL('/');
	});

	userTest('記録を直す・消す', async ({ page }) => {
		const timeZone = await fetchTimeZone(page.request);
		const day = latestPastMorningDay(timeZone);
		const id = await createRecordByApi(page.request, {
			localMeasuredAt: `${day}T07:00`,
			systolic: 120,
			diastolic: 80,
			pulse: 70
		});
		const home = new HomePage(page);
		await home.open();
		const pulseText = ja.record_pulse_with_value.replace('{pulse}', '70');
		const card = home.recordCard(pulseText).root;
		await expect(card).toContainText(/120\s*\/\s*80/);
		await takeHelpShot(page, 'edit-record-1', [home.recordCard(pulseText).editButton]);

		const sheet = await home.openRecord(pulseText);
		const { systolic } = sheet.fields;
		await expect(systolic).toHaveValue('120');
		await systolic.fill('122');
		await takeHelpShot(page, 'edit-record-2', [systolic, sheet.updateButton]);
		const updated = waitForApiResponse(page, 'PUT', `/records/${id}`);
		await sheet.updateButton.click();
		expect((await updated).ok()).toBe(true);
		await expect(sheet.root).toBeHidden();
		await expect(card).toContainText(/122\s*\/\s*80/);

		await blurFocus(page);
		await takeHelpShot(page, 'edit-record-3', [home.recordCard(pulseText).deleteButton]);
		const confirm = await home.openDelete(pulseText);
		await takeHelpShot(page, 'edit-record-4', [confirm.deleteButton]);
		const deleted = waitForApiResponse(page, 'DELETE', `/records/${id}`);
		await confirm.deleteButton.click();
		expect((await deleted).ok()).toBe(true);
		await expect(confirm.root).toBeHidden();
		await expect(card).toHaveCount(0);
	});

	userTest('記録を見る', async ({ page }) => {
		const home = await openWeekWithPastRecords(page);
		const { periodNav, chart } = home;

		const { weekButton: week, monthButton: month } = periodNav;
		await takeHelpShot(page, 'view-records-1', [week, month]);
		await month.click();
		await expect(month).toHaveAttribute('aria-pressed', 'true');
		await week.click();
		await expect(week).toHaveAttribute('aria-pressed', 'true');
		await chart.waitForLines(4);

		const { prevButton: prev, nextButton: next, thisWeekButton: thisWeek } = periodNav;
		const firstRecord = home.recordCard('起床後').root;
		await takeHelpShot(page, 'view-records-2', [prev, thisWeek, next]);
		// 記録のある週が今週とは限らないので、「今週へ」ではなく次へで戻る。
		await prev.click();
		await expect(firstRecord).toHaveCount(0);
		await next.click();
		await expect(firstRecord).toBeVisible();
		await chart.waitForLines(4);

		await takeHelpShot(page, 'view-records-3', [chart.legend, chart.area]);
		await chart.seriesToggle(ja.chart_morning_systolic_label).click();
		await expect(chart.lines).toHaveCount(3);
		await chart.seriesToggle(ja.chart_morning_systolic_label).click();
		await chart.waitForLines(4);

		const morning = home.stats.averageCard(ja.bp_stats_morning_average_label);
		const evening = home.stats.averageCard(ja.bp_stats_evening_average_label);
		await expect(morning).toContainText(/\d+\s*\/\s*\d+/);
		await takeHelpShot(page, 'view-records-4', [morning, evening]);
	});

	userTest('印刷する', async ({ page }) => {
		const printCalls = await countPrints(page);
		const home = await openWeekWithPastRecords(page);
		// 開いたままだと、入口が下端の記録のボタンの帯に隠れるため、画面の中ほどまで送る。
		await home.reportLink.evaluate((el) => el.scrollIntoView({ block: 'center' }));
		await takeHelpShot(page, 'print-report-1', [home.reportLink]);

		const report = await home.openReport();
		// 読み込み中の表示や、押せない「印刷する」を撮らないよう、グラフが描けるまで待つ。
		await report.chart.waitForLines(4);
		const { prevButton: prev, nextButton: next, thisWeekButton: period } = report.periodNav;
		await takeHelpShot(page, 'print-report-2', [prev, period, next]);

		await takeHelpShot(page, 'print-report-3', [report.printButton]);
		expect(await printCalls()).toBe(0);
		await report.printButton.click();
		expect(await printCalls()).toBe(1);
	});

	userTest('朝と夜の時間帯を変える', async ({ page, throwawayUser: user }) => {
		await resetSettingsByApi(page.request, 'Asia/Tokyo');
		const settings = await openSettings(page, 'settings-period-1');
		const accountId = settings.username(user.username);
		const picker = settings.morningEnd;
		const morningEnd = picker.trigger;
		// 欄の名前は見えない文言として同じボタンに入っているため、時刻は末尾だけを見る。
		await expect(morningEnd).toHaveText(/\s10:00$/);
		await takeHelpShot(page, 'settings-period-2', [morningEnd], [accountId]);

		await morningEnd.click();
		const close = picker.closeButton;
		await takeHelpShot(
			page,
			'settings-period-3',
			[picker.hours, picker.minutes, close],
			[accountId]
		);
		const saved = waitForApiResponse(page, 'PUT', '/settings');
		await picker.hourButton(9).click();
		expect((await saved).ok()).toBe(true);
		await close.click();
		await expect(morningEnd).toHaveText(/\s9:00$/);
	});

	userTest('タイムゾーンを変える', async ({ page, throwawayUser: user }) => {
		// 時刻がずれている人が直す場面にするため、日本以外から始める。
		await resetSettingsByApi(page.request, 'UTC');
		const settings = await openSettings(page, 'settings-timezone-1');
		const accountId = settings.username(user.username);
		const timezone = settings.timeZone;
		await expect(timezone).toHaveValue('UTC');
		// 「自動設定」は、選ぶ前から選んだら適用されるゾーン (playwright.config.ts の Asia/Tokyo) を名乗る。
		await expect(settings.autoTimeZoneOption).toContainText('Asia/Tokyo');
		await takeHelpShot(page, 'settings-timezone-2', [timezone], [accountId]);

		const saved = waitForApiResponse(page, 'PUT', '/settings');
		await timezone.selectOption({ index: 0 });
		expect((await saved).ok()).toBe(true);
		await expect.poll(() => fetchTimeZone(page.request)).toBe('Asia/Tokyo');
		await takeHelpShot(page, 'settings-timezone-3', [timezone], [accountId]);
	});

	userTest('画面の明るさを変える', async ({ page }) => {
		const home = new HomePage(page);
		await home.open();
		const { modeToggle } = home.header;
		await takeHelpShot(page, 'theme-1', [modeToggle.button]);

		await modeToggle.button.click();
		const choice = (label: string) => modeToggle.option(label);
		await expect(choice(ja.theme_system)).toBeVisible();
		await takeHelpShot(page, 'theme-2', [choice(ja.theme_light), choice(ja.theme_dark)]);
		await takeHelpShot(page, 'theme-3', [choice(ja.theme_system)]);
		await choice(ja.theme_system).click();
		await expect(modeToggle.menu).toBeHidden();
	});

	userTest('記録を書き出す', async ({ page, throwawayUser: user }) => {
		const settings = await openSettings(page, 'export-csv-1');
		await takeHelpShot(
			page,
			'export-csv-2',
			[settings.exportCsvButton],
			[settings.username(user.username)]
		);
		// 手順3のファイル名の説明どおりに書き出せること。
		const file = await settings.exportCsv();
		expect(file.suggestedFilename()).toBe(
			`bp-records-${todayIn(await fetchTimeZone(page.request))}.csv`
		);
	});

	userTest('CSV ファイルから記録を取り込む', async ({ page, throwawayUser: user }) => {
		const today = todayIn(await fetchTimeZone(page.request));
		const firstDay = addDays(today, -3);
		await createRecordsByApi(page.request, threeDaysOfMorningAndEvening(firstDay));
		const settings = await openSettings(page, 'import-csv-1');
		const accountId = settings.username(user.username);
		const csv = await readFile(await (await settings.exportCsv()).path());
		// 書き出した後に入れた記録は、その日を取り込むと消える。確認画面で「削除」と出るところを見せる。
		await createRecordByApi(page.request, {
			localMeasuredAt: `${firstDay}T12:00`,
			systolic: 118,
			diastolic: 76,
			pulse: 66
		});

		await takeHelpShot(page, 'import-csv-2', [settings.importButton], [accountId]);
		const importPage = await settings.pickCsv({
			name: 'records.csv',
			mimeType: 'text/csv',
			buffer: csv
		});

		// 読み込んだ行をひととおり見てから確認へ進む。
		await expect(importPage.reviewButton).toBeEnabled();
		await takeHelpShot(page, 'import-csv-3', [importPage.reviewButton]);
		await importPage.reviewButton.click();
		// 書き出した記録は変更なし、後から入れた記録だけ削除。
		await expect(importPage.preview.heading).toBeVisible();
		await expect(page.getByText(ja.import_preview_status_unchanged, { exact: true })).toHaveCount(
			6
		);
		const removed = page.getByText(ja.import_preview_status_removed, { exact: true });
		await expect(removed).toHaveCount(1);
		const confirm = importPage.preview.confirmButton;
		await expect(confirm).toBeEnabled();
		await takeHelpShot(page, 'import-csv-4', [removed, confirm]);
		const imported = waitForApiResponse(page, 'POST', '/records/import');
		await confirm.click();
		expect((await imported).ok()).toBe(true);
		await page.waitForURL('/settings');
	});

	userTest('アカウントを削除する', async ({ page, throwawayUser: user }) => {
		const settings = await openSettings(page, 'delete-account-1');
		// 設定画面は、ダイアログの後ろに透けても使い捨てユーザーの ID が写る。
		const accountId = settings.username(user.username);
		await settings.deleteAccountButton.scrollIntoViewIfNeeded();
		await takeHelpShot(page, 'delete-account-2', [settings.deleteAccountButton], [accountId]);

		const dialog = await settings.openDeleteAccount();
		await dialog.confirmation.fill(ja.delete_account_dialog_confirm_word);
		await takeHelpShot(page, 'delete-account-3', [dialog.confirmation], [accountId]);

		await expect(dialog.submitButton).toBeEnabled();
		await takeHelpShot(page, 'delete-account-4', [dialog.submitButton], [accountId]);
		await dialog.submitButton.click();
		await expect(dialog.scheduledResult).toBeVisible();
		await takeHelpShot(page, 'delete-account-5', [], [accountId]);
	});

	userTest('ログインできないとき', async ({ page }) => {
		// 使い捨てユーザーは作った時点でログイン済みなので、ログアウトした状態に戻す。
		await clearLogin(page);
		const login = new LoginPage(page);
		await login.open();
		await login.username.fill(SAMPLE_TEXT);
		await login.password.fill('password');
		const { showPasswordButton: show } = login;
		await takeHelpShot(page, 'trouble-login-1', [show]);
		await takeHelpShot(page, 'trouble-login-2', [login.forgotPasswordLink]);
		await show.click();
		await expect(login.hidePasswordButton).toBeVisible();

		const resetPassword = await login.openForgotPassword();
		await resetPassword.email.fill(SAMPLE_TEXT);
		await takeHelpShot(page, 'trouble-login-3', [resetPassword.email, resetPassword.submitButton]);
	});

	userTest('LINE でメールアドレスを受け取れないと言われたとき', async ({ page }) => {
		await clearLogin(page);
		const login = new LoginPage(page);
		await login.open();
		// 1・3歩目は LINE の画面なので、画像は撮らない。
		await takeHelpShot(page, 'trouble-line-email-2', [login.lineButton]);
	});

	userTest('LINE のリンクなどから開いてログインできないとき', async ({ page }) => {
		await clearLogin(page);
		// 内蔵ブラウザは User-Agent の末尾の `Line/<版>` で見分ける (docs/authentication.md)。
		await page.addInitScript(() => {
			Object.defineProperty(navigator, 'userAgent', {
				value: `${navigator.userAgent} Line/15.0.0`,
				configurable: true
			});
		});
		const login = new LoginPage(page);
		await login.open();
		// 1・2歩目は LINE などの画面なので、画像は撮らない。
		await takeHelpShot(page, 'trouble-in-app-browser-3', [login.copyLinkButton]);
	});

	userTest('写真が読み取れないとき', async ({ page }) => {
		await mockOcrExtract(page, { json: noValuesResult() });
		const home = new HomePage(page);
		await home.open();
		const photo = await samplePhoto(page, { kind: 'monitor' });
		const photoPage = await home.pickPhoto(photo);
		await expect(photoPage.noValuesMessage).toBeVisible();
		await takeHelpShot(page, 'trouble-photo-1', [photoPage.retakeButton]);

		await mockOcrExtract(page, errorResponse(429, 'ocr_budget_exhausted'));
		await photoPage.retake(photo);
		const limit = photoPage.quotaExhaustedTitle;
		await expect(limit).toBeVisible();
		await takeHelpShot(page, 'trouble-photo-2', [limit]);
		await photoPage.back();

		await blurFocus(page);
		await takeHelpShot(page, 'trouble-photo-3', [home.addRecordButton]);
	});

	userTest('無料の読み取りを使い切ったとき', async ({ page }) => {
		await mockOcrStatus(page, { quotas: [freeQuota(0)], quotaLow: true, topupAvailable: true });
		const home = new HomePage(page);
		await home.open();
		const photoPage = await home.choosePhoto(await samplePhoto(page, { kind: 'monitor' }));
		await expect(photoPage.quotaExhaustedTitle).toBeVisible();
		await takeHelpShot(page, 'trouble-ocr-budget-1', [photoPage.topupButton]);

		// 「同意して購入へ進む」は Stripe へ移るので押さない。3歩目は Stripe の画面なので、画像は撮らない。
		const topup = await photoPage.openTopup();
		await expect(topup.agreeButton).toBeVisible();
		await takeHelpShot(page, 'trouble-ocr-budget-2', [topup.agreeButton]);

		await page.route('**/api/v1/payments/ocr-topup/result**', (route) =>
			route.fulfill({ json: { status: 'succeeded' } })
		);
		const result = new TopupResultPage(page);
		await result.open('cs_help');
		await expect(result.succeededTitle).toBeVisible();
		const back = result.backToPhotoButton;
		await takeHelpShot(page, 'trouble-ocr-budget-4', [back]);
	});
});
