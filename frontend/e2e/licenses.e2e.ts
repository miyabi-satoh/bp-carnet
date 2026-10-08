// 第三者のソフトウェア (docs/third-party-licenses.md)。
// 一覧はビルドで作るので、e2e はビルドした画面に当てて確かめる。
import { test, expect, NO_LOGIN } from './fixtures';
import { ja } from './helpers';
import { LicensesPage } from './pages/licenses-page';

test.use({ storageState: NO_LOGIN });

test('サインインしなくても開け、画面に入っている部品の行を開くと、ソースの置き場所と条文が出る', async ({
	page
}) => {
	const licenses = new LicensesPage(page);
	await licenses.open();

	await expect(licenses.heading).toHaveText(ja.licenses_title);
	const svelte = licenses.packageItem(licenses.webSection, 'svelte');
	await expect(svelte).toContainText('MIT');
	await licenses.openPackage(svelte);
	await expect(licenses.sourceLink(svelte)).toHaveAttribute(
		'href',
		'https://github.com/sveltejs/svelte'
	);
	await expect(svelte).toContainText('Permission is hereby granted');
});

test('ブラウザでは iPhone アプリの節を出さない', async ({ page }) => {
	const licenses = new LicensesPage(page);
	await licenses.open();

	await expect(licenses.webSection).toBeVisible();
	await expect(licenses.iosSection).toHaveCount(0);
});
