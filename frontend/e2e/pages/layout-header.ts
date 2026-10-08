import type { Locator, Page } from '@playwright/test';
import { ja } from '../helpers';
import { LogoutDialog } from './logout-dialog';
import { ModeToggle } from './mode-toggle';

/** ログイン後の画面に共通のヘッダー。コンポーネントに分かれておらず、routes/+layout.svelte に直に書かれている。 */
export class LayoutHeader {
	readonly modeToggle: ModeToggle;

	constructor(private readonly page: Page) {
		this.modeToggle = new ModeToggle(page);
	}

	/** ロゴの横の「β」バッジ (オープンβテスト中の構成だけ)。 */
	get betaBadge(): Locator {
		return this.page.getByRole('img', { name: ja.beta_badge_label, exact: true });
	}

	get userMenuButton(): Locator {
		return this.page.getByRole('button', { name: ja.layout_user_menu_label });
	}

	/** 開いたメニューの項目。`name` は `ja.settings_title` など。 */
	menuItem(name: string): Locator {
		return this.page.getByRole('menuitem', { name, exact: true });
	}

	/** ユーザーのメニューを開いて `name` の項目を選ぶ。 */
	async chooseUserMenuItem(name: string): Promise<void> {
		await this.userMenuButton.click();
		await this.menuItem(name).click();
	}

	/** ユーザーのメニューの「ログアウト」から確認を開く。 */
	async openLogout(): Promise<LogoutDialog> {
		await this.chooseUserMenuItem(ja.common_logout_button);
		return new LogoutDialog(this.page);
	}
}
