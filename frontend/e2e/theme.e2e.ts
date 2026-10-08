// ライト/ダーク/システム追従のテーマ切り替え。入口は共通ヘッダーのボタン。
// テーマはブラウザの中にだけ保存され、サーバーのデータを変えないため、共有の管理者で流す。
import type { Page } from '@playwright/test';
import { test, expect } from './fixtures';
import { ja } from './helpers';
import { HomePage } from './pages/home-page';

/** ヘッダーのテーマのメニューを開き、`label` (`ja.theme_dark` など) を選ぶ。 */
function chooseTheme(page: Page, label: string): Promise<void> {
	return new HomePage(page).header.modeToggle.choose(label);
}

/** メニューを開き、`label` が選ばれていることを確かめて閉じる。 */
async function expectChosenTheme(page: Page, label: string): Promise<void> {
	const { modeToggle } = new HomePage(page).header;
	await modeToggle.button.click();
	await expect(modeToggle.option(label)).toBeChecked();
	await page.keyboard.press('Escape');
	await expect(modeToggle.menu).toBeHidden();
}

function bodyBackground(page: Page): Promise<string> {
	return page.evaluate(() => getComputedStyle(document.body).backgroundColor);
}

test('ダーク・ライトを選ぶと表示が切り替わり、読み込み直しても保たれる', async ({ page }) => {
	await page.emulateMedia({ colorScheme: 'light' });
	await new HomePage(page).open();
	const html = page.locator('html');
	await expect(html).not.toHaveClass(/\bdark\b/);
	const lightBackground = await bodyBackground(page);

	// OS の設定がライトでも、ダークを選べばダークになる。
	await chooseTheme(page, ja.theme_dark);
	await expect(html).toHaveClass(/\bdark\b/);
	await expect.poll(() => bodyBackground(page)).not.toBe(lightBackground);

	await page.reload();
	await expect(html).toHaveClass(/\bdark\b/);
	await expectChosenTheme(page, ja.theme_dark);

	await chooseTheme(page, ja.theme_light);
	await expect(html).not.toHaveClass(/\bdark\b/);
	await expect.poll(() => bodyBackground(page)).toBe(lightBackground);

	await page.reload();
	await expect(html).not.toHaveClass(/\bdark\b/);
	await expectChosenTheme(page, ja.theme_light);
});

test('システムを選ぶと、OS の配色の設定に追従する', async ({ page }) => {
	await page.emulateMedia({ colorScheme: 'light' });
	await new HomePage(page).open();
	const html = page.locator('html');
	await chooseTheme(page, ja.theme_dark);
	await expect(html).toHaveClass(/\bdark\b/);

	await chooseTheme(page, ja.theme_system);
	await expect(html).not.toHaveClass(/\bdark\b/);

	// 開いたままでも、OS の設定を変えると追従する。
	await page.emulateMedia({ colorScheme: 'dark' });
	await expect(html).toHaveClass(/\bdark\b/);

	await page.reload();
	await expect(html).toHaveClass(/\bdark\b/);
	await expectChosenTheme(page, ja.theme_system);

	await page.emulateMedia({ colorScheme: 'light' });
	await expect(html).not.toHaveClass(/\bdark\b/);
});
