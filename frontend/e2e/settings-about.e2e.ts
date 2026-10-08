// 設定画面の「このアプリについて」。
import { test, expect } from './fixtures';
import { ja } from './helpers';
import { SettingsPage } from './pages/settings-page';

// e2e の backend の設定 (scripts/backend-process.ts) の問い合わせ先。
const CONTACT_URL = 'https://example.com/contact/';

test('使い方・規約・プライバシーポリシー・第三者のソフトウェア・問い合わせ先へのリンクを並べ、問い合わせ先は別のタブで開く', async ({
	page
}) => {
	const settings = new SettingsPage(page);
	await settings.open();

	await expect(settings.aboutLink(ja.terms_title)).toHaveAttribute('href', '/terms');
	await expect(settings.aboutLink(ja.privacy_title)).toHaveAttribute('href', '/privacy');
	await expect(settings.aboutLink(ja.licenses_title)).toHaveAttribute('href', '/licenses');
	await expect(settings.aboutLink(ja.help_title)).toHaveAttribute('href', '/help');
	const contact = settings.aboutLink(ja.settings_contact_link);
	await expect(contact).toHaveAttribute('href', CONTACT_URL);
	await expect(contact).toHaveAttribute('target', '_blank');
});

test('問い合わせ先を問い合わせられなければ、問い合わせ先の行だけを出さない', async ({ page }) => {
	await page.route('**/api/v1/auth/providers', (route) => route.fulfill({ status: 503 }));
	const settings = new SettingsPage(page);
	await settings.open();

	await expect(settings.aboutLink(ja.help_title)).toBeVisible();
	await expect(settings.aboutLinks).toHaveCount(4);
});
