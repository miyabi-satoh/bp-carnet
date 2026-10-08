import type { Locator, Page } from '@playwright/test';
import { ja } from '../helpers';
import { AdminAddUserDialog } from './admin-add-user-dialog';
import { AdminDeleteUserDialog } from './admin-delete-user-dialog';
import { AdminOcrLimitDialog } from './admin-ocr-limit-dialog';
import { AdminResetPasswordDialog } from './admin-reset-password-dialog';
import { AdminUserDetailPage } from './admin-user-detail-page';

/** 管理者のユーザー一覧 (`/admin/users`)。ユーザーごとのカードに、状態と操作のボタンが並ぶ。 */
export class AdminUsersPage {
	constructor(private readonly page: Page) {}

	async open(): Promise<void> {
		await this.page.goto('/admin/users');
	}

	get addButton(): Locator {
		return this.page.getByRole('button', { name: ja.admin_users_add_button, exact: true });
	}

	/** 「追加...」からユーザーの追加を開く。 */
	async openAddUser(): Promise<AdminAddUserDialog> {
		await this.addButton.click();
		return new AdminAddUserDialog(this.page);
	}

	/** 一覧を絞り込む検索の欄。 */
	get search(): Locator {
		return this.page.getByRole('searchbox', { name: ja.admin_users_search_label, exact: true });
	}

	/** 「全N人」。ほかのテストのユーザーも一覧に載るので、全体の人数は問わない。 */
	get totalCount(): Locator {
		return this.page.getByText(new RegExp(`^${ja.admin_users_count.replace('{total}', '\\d+')}$`));
	}

	/** 「全N人中{shown}人」。 */
	filteredCount(shown: number): Locator {
		const text = ja.admin_users_filtered_count
			.replace('{total}', '\\d+')
			.replace('{shown}', String(shown));
		return this.page.getByText(new RegExp(`^${text}$`));
	}

	get noMatch(): Locator {
		return this.page.getByText(ja.admin_users_no_match, { exact: true });
	}

	/** 一覧のカードすべて。 */
	get userCards(): Locator {
		return this.page.getByRole('listitem');
	}

	/** そのユーザーのカード。 */
	userCard(username: string): Locator {
		return this.page.getByRole('listitem').filter({ hasText: username });
	}

	freezeButton(username: string): Locator {
		return this.cardButton(username, ja.admin_users_freeze_button);
	}

	unfreezeButton(username: string): Locator {
		return this.cardButton(username, ja.admin_users_unfreeze_button);
	}

	cancelDeletionButton(username: string): Locator {
		return this.cardButton(username, ja.admin_users_cancel_deletion_button);
	}

	/** 「OCR上限を変更...」。閉じた後に開き直せるかを確かめるため、ボタンも出しておく。 */
	ocrLimitButton(username: string): Locator {
		return this.userCard(username).getByRole('button', {
			name: ja.admin_users_ocr_limit_edit_label.replace('{name}', username)
		});
	}

	/** 削除の予約を開く。削除は凍結中のユーザーにだけ出る。 */
	async openDeleteUser(username: string): Promise<AdminDeleteUserDialog> {
		await this.userCard(username)
			.getByRole('button', { name: ja.admin_users_delete_label.replace('{name}', username) })
			.click();
		return new AdminDeleteUserDialog(this.page, username);
	}

	async openOcrLimit(username: string): Promise<AdminOcrLimitDialog> {
		await this.ocrLimitButton(username).click();
		return new AdminOcrLimitDialog(this.page);
	}

	async openResetPassword(username: string): Promise<AdminResetPasswordDialog> {
		await this.cardButton(username, ja.admin_users_reset_password_button).click();
		return new AdminResetPasswordDialog(this.page);
	}

	/** カードのリンクからユーザーの閲覧ページへ移る。 */
	async openDetail(username: string, id: number): Promise<AdminUserDetailPage> {
		await this.userCard(username).getByRole('link').click();
		await this.page.waitForURL(`/admin/users/${id}`);
		return new AdminUserDetailPage(this.page);
	}

	private cardButton(username: string, name: string): Locator {
		return this.userCard(username).getByRole('button', { name, exact: true });
	}
}
