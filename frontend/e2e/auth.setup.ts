import { mkdir } from 'node:fs/promises';
import path from 'node:path';
import { expect, test as setup } from '@playwright/test';
import { ADMIN_USERNAME, ADMIN_PASSWORD, ADMIN_STORAGE_STATE } from './auth';
import { HomePage } from './pages/home-page';
import { LoginPage } from './pages/login-page';

setup('管理者としてログインする', async ({ page }) => {
	await new LoginPage(page).logIn(ADMIN_USERNAME, ADMIN_PASSWORD);
	await page.waitForURL('/');
	await expect(new HomePage(page).heading).toBeVisible();
	// 初回 (e2e/.auth がまだ無い) の `just e2e` でも保存に失敗しないよう、保存先を先に作っておく。
	await mkdir(path.dirname(ADMIN_STORAGE_STATE), { recursive: true });
	await page.context().storageState({ path: ADMIN_STORAGE_STATE });
});
