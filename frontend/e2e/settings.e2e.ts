// 設定 (朝・夜の時間帯・タイムゾーン・パスワード)。ユーザーの状態を変えるため、使い捨てユーザーで流す。
import type { Route } from '@playwright/test';
import {
	createRecordsByApi,
	fetchTimeZone,
	loginStatus,
	resetSettingsByApi,
	send
} from './api-helpers';
import { addDays, todayIn } from './date-helpers';
import { userTest as test, expect, NO_LOGIN } from './fixtures';
import { clearLogin, ja, waitForApiResponse } from './helpers';
import { freeQuota, mockOcrStatus, paidQuota } from './ocr-mocks';
import { HomePage } from './pages/home-page';
import { LoginPage } from './pages/login-page';
import { SettingsPage } from './pages/settings-page';

test.use({ storageState: NO_LOGIN });

test('朝・夜の時間帯とタイムゾーンを変えると、表示と集計に反映される', async ({ page }) => {
	// 期待値を決めるため、サーバーの既定値に頼らず始めの設定を決めておく。
	await resetSettingsByApi(page.request, 'Asia/Tokyo');
	// 週の境目にかからないよう、「今週」ではなく期間を指定して見る。
	const day = addDays(todayIn('Asia/Tokyo'), -1);
	await createRecordsByApi(page.request, [
		{ localMeasuredAt: `${day}T07:00`, systolic: 120, diastolic: 80, memo: '起床後' },
		{ localMeasuredAt: `${day}T09:00`, systolic: 140, diastolic: 100, memo: '散歩のあと' },
		{ localMeasuredAt: `${day}T13:00`, systolic: 100, diastolic: 60, memo: '昼食後' }
	]);

	const settings = new SettingsPage(page);
	const home = new HomePage(page);
	await settings.open();
	const morningEnd = settings.morningEnd.trigger;
	// 欄の名前は見えない文言として同じボタンに入っているため、時刻は末尾だけを見る。
	await expect(morningEnd).toHaveText(/\s10:00$/);

	// 朝の終わりを 10:00 → 8:00 にすると、9:00 の記録は朝から外れる。
	await morningEnd.click();
	const saved = waitForApiResponse(page, 'PUT', '/settings');
	await settings.morningEnd.hourButton(8).click();
	expect((await saved).ok()).toBe(true);
	await settings.morningEnd.closeButton.click();
	await expect(morningEnd).toHaveText(/\s8:00$/);

	await home.open();
	await home.periodNav.filter(day, day);
	await expect(home.stats.averageCard(ja.bp_stats_morning_average_label)).toContainText(
		/120\s*\/\s*80/
	);
	await expect(home.recordCard('起床後').root).toContainText(
		`${ja.record_day_period_morning_label} 7:00`
	);
	await expect(home.recordCard('散歩のあと').root).not.toContainText(
		ja.record_day_period_morning_label
	);

	// タイムゾーンを UTC (日本時間の9時間前) にすると、時刻と区分が変わる。
	// 7:00 → 前日の 22:00 (夜)、9:00 → 0:00 (どちらでもない)、13:00 → 4:00 (朝)。
	await settings.open();
	const timezoneSaved = waitForApiResponse(page, 'PUT', '/settings');
	await settings.timeZone.selectOption('Etc/UTC');
	expect((await timezoneSaved).ok()).toBe(true);
	await expect(settings.savedNotice(ja.settings_timezone_section_title)).toBeVisible();
	await expect(settings.savedNotice(ja.settings_period_section_title)).toBeHidden();

	await home.open();
	await home.periodNav.filter(addDays(day, -1), day);
	await expect(home.recordCard('起床後').root).toContainText(
		`${ja.record_day_period_evening_label} 22:00`
	);
	await expect(home.recordCard('散歩のあと').root).toContainText('0:00');
	await expect(home.recordCard('昼食後').root).toContainText(
		`${ja.record_day_period_morning_label} 4:00`
	);
	await expect(home.stats.averageCard(ja.bp_stats_morning_average_label)).toContainText(
		/100\s*\/\s*60/
	);
	await expect(home.stats.averageCard(ja.bp_stats_evening_average_label)).toContainText(
		/120\s*\/\s*80/
	);
});

test('保存を送っている間に別の欄を変えても、「保存しました」は保存できた欄にだけ出す', async ({
	page
}) => {
	await resetSettingsByApi(page.request, 'Asia/Tokyo');
	// 設定の保存 (PUT) を1件ずつ止めておき、こちらの合図で通す。
	const held: Route[] = [];
	await page.route('**/api/v1/settings', (route) =>
		route.request().method() === 'PUT' ? void held.push(route) : route.continue()
	);
	const settings = new SettingsPage(page);
	await settings.open();

	await settings.timeZone.selectOption('Etc/UTC');
	await expect.poll(() => held.length).toBe(1);
	// タイムゾーンを送っている間に、朝の時間帯を変える (送るのは前の保存が終わってから)。
	await settings.morningEnd.trigger.click();
	await settings.morningEnd.hourButton(8).click();
	await settings.morningEnd.closeButton.click();

	await held[0].continue();
	await expect(settings.savedNotice(ja.settings_timezone_section_title)).toBeVisible();
	await expect.poll(() => held.length).toBe(2);
	await expect(settings.savedNotice(ja.settings_period_section_title)).toBeHidden();

	await held[1].continue();
	await expect(settings.savedNotice(ja.settings_period_section_title)).toBeVisible();
});

test('パスワードを変えると、新しいパスワードでログインし直せる', async ({
	page,
	throwawayUser: user
}) => {
	const newPassword = `${user.password}-new`;

	const settings = new SettingsPage(page);
	await settings.open();
	const changePassword = await settings.openChangePassword();
	await changePassword.currentPassword.fill(user.password);
	await changePassword.newPassword.fill(newPassword);
	await changePassword.newPasswordConfirmation.fill(newPassword);
	// 変更は Argon2 を2回回し (現在のパスワードの照合と新しいハッシュ)、debug ビルドの並列実行では
	// expect の待ち時間を超えうるため、応答そのものを待つ。
	const changed = waitForApiResponse(page, 'PUT', '/account/password');
	await changePassword.submitButton.click();
	expect((await changed).status()).toBe(204);
	// 後片付け (→ fixtures.ts) は、このユーザーでログインし直して記録を消すため、今のパスワードを伝える。
	const oldPassword = user.password;
	user.password = newPassword;
	await expect(changePassword.doneTitle).toBeVisible();

	// ログアウトした状態から入り直す。古いパスワードは通らない。
	await clearLogin(page);
	expect(await loginStatus(page.request, user.username, oldPassword)).toBe(401);

	const home = await new LoginPage(page).logIn(user.username, newPassword);
	await expect(home.heading).toBeVisible();
});

test.describe('ブラウザのタイムゾーンが個人設定と違う端末', () => {
	test.use({ timezoneId: 'America/New_York' });

	test('自動設定なら画面を開くとブラウザのタイムゾーンに揃い、手動にすると揃えない', async ({
		page
	}) => {
		const home = new HomePage(page);
		const settings = new SettingsPage(page);
		await home.open();
		await expect.poll(() => fetchTimeZone(page.request)).toBe('America/New_York');

		await settings.open();
		await expect(settings.autoTimeZoneOption).toContainText('America/New_York');

		// 手動で日本にしたら、ブラウザのタイムゾーンが違っても開き直したときに変えない。
		const saved = waitForApiResponse(page, 'PUT', '/settings');
		await settings.timeZone.selectOption('Asia/Tokyo');
		expect((await saved).ok()).toBe(true);
		await home.open();
		expect(await fetchTimeZone(page.request)).toBe('Asia/Tokyo');
	});
});

// 同意は設定からいつでも取り消せる (App Store Review Guidelines 5.1.1(ii)、docs/ocr.md)。
test('写真を送ることへの同意を、設定から取り消せる', async ({ page }) => {
	const settings = new SettingsPage(page);
	await settings.open();
	// 同意していなければ、取り消すものが無いので欄を出さない。
	await expect(settings.account).toBeVisible();
	await expect(settings.ocrSection).toBeHidden();

	await send(page.request, 'post', '/ocr/consent');
	await page.reload();
	const withdrawn = waitForApiResponse(page, 'DELETE', '/ocr/consent');
	await settings.withdrawOcrConsentButton.click();
	expect((await withdrawn).status()).toBe(204);
	await expect(settings.ocrConsentWithdrawnNotice).toBeVisible();
	await expect(settings.ocrSection).toBeHidden();
	expect(await send(page.request, 'get', '/ocr/status')).toMatchObject({ consented: false });
});

// 読み取りの枠を使う順に並べ、残りによらず買い足せる (docs/ocr.md)。
test('設定の「写真の読み取り」に、枠を使う順に並べ、いつでも買い足せる', async ({ page }) => {
	await mockOcrStatus(page, {
		consented: false,
		quotas: [paidQuota(80, '2026-09-27T03:00:00.000Z'), freeQuota(12)],
		topupAvailable: true
	});
	const settings = new SettingsPage(page);
	await settings.open();
	const section = settings.ocrSection;
	await expect(section.getByText(ja.ocr_quota_paid_title, { exact: true })).toBeVisible();
	await expect(section.getByText('2026年9月27日に購入', { exact: true })).toBeVisible();
	await expect(section.getByText(ja.ocr_quota_free_note, { exact: true })).toBeVisible();
	await expect(section.getByText('残り 12%', { exact: true })).toBeVisible();
	await expect(settings.withdrawOcrConsentButton).toHaveCount(0);

	const topup = await settings.openTopup();
	await expect(topup.root).toBeVisible();
});
