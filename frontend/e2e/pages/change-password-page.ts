import type { Locator, Page } from '@playwright/test';
import { ja, requiredLabel } from '../helpers';

/** パスワードの変更 (`/settings/change-password`)。 */
export class ChangePasswordPage {
	constructor(private readonly page: Page) {}

	get currentPassword(): Locator {
		return this.page.getByLabel(requiredLabel(ja.settings_change_password_current_label), {
			exact: true
		});
	}

	get newPassword(): Locator {
		return this.page.getByLabel(requiredLabel(ja.common_new_password_label), { exact: true });
	}

	get newPasswordConfirmation(): Locator {
		return this.page.getByLabel(requiredLabel(ja.common_new_password_confirm_label), {
			exact: true
		});
	}

	get submitButton(): Locator {
		return this.page.getByRole('button', {
			name: ja.settings_change_password_submit_button,
			exact: true
		});
	}

	get doneTitle(): Locator {
		return this.page.getByText(ja.settings_change_password_done_title);
	}
}
