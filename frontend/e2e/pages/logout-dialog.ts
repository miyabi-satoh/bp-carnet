import type { Locator, Page } from '@playwright/test';
import { ja } from '../helpers';
import { LoginPage } from './login-page';

/** ログアウトの確認 (logout-dialog.svelte)。 */
export class LogoutDialog {
	readonly root: Locator;

	constructor(private readonly page: Page) {
		this.root = page.getByRole('alertdialog', { name: ja.logout_dialog_title });
	}

	get confirmButton(): Locator {
		return this.root.getByRole('button', { name: ja.logout_dialog_confirm_button, exact: true });
	}

	/** 確認ダイアログは × を出さない。無いことを確かめるために探す。 */
	get closeButton(): Locator {
		return this.root.getByRole('button', { name: ja.common_close_button, exact: true });
	}

	/** ログアウトして、ログイン画面へ移るのを待つ。 */
	async confirm(): Promise<LoginPage> {
		await this.confirmButton.click();
		await this.page.waitForURL('/login');
		return new LoginPage(this.page);
	}

	/** ログアウトできなかったことを知らせるダイアログ。 */
	get errorDialog(): Locator {
		return this.page.getByRole('alertdialog', { name: ja.common_logout_error_title });
	}

	async closeError(): Promise<void> {
		await this.errorDialog
			.getByRole('button', { name: ja.common_close_button, exact: true })
			.click();
	}

	get cancelButton(): Locator {
		return this.root.getByRole('button', { name: ja.common_cancel_button, exact: true });
	}
}
