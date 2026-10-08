import type { Locator, Page } from '@playwright/test';
import { ja } from '../helpers';
import { BpStatsSummary } from './bp-stats-summary';
import { RecordCard } from './record-card';

/** 管理者によるユーザーの閲覧 (`/admin/users/[id]`)。アカウント情報と記録を読み取り専用で出す。 */
export class AdminUserDetailPage {
	readonly stats: BpStatsSummary;

	constructor(private readonly page: Page) {
		this.stats = new BpStatsSummary(page);
	}

	/** ページの見出し (ユーザー ID)。 */
	heading(username: string): Locator {
		return this.page.getByRole('heading', { name: username, exact: true });
	}

	/** 閲覧が記録に残ることの断り書き。 */
	get auditNotice(): Locator {
		return this.page.getByText(ja.admin_user_detail_audit_notice);
	}

	/** アカウント情報の行。`label` は `ja.admin_user_detail_status_label` など。 */
	accountRow(label: string): Locator {
		return this.page
			.getByRole('region', { name: ja.admin_user_detail_account_section_title })
			.locator('div')
			.filter({ hasText: label });
	}

	get records(): Locator {
		return this.page.getByRole('region', { name: ja.admin_user_detail_records_section_title });
	}

	/** 記録カードの直すボタン。読み取り専用なので出ない。無いことを確かめるために探す。 */
	get recordEditButtons(): Locator {
		return this.records.getByRole('button', { name: RecordCard.EDIT_LABEL });
	}

	get recordDeleteButtons(): Locator {
		return this.records.getByRole('button', { name: RecordCard.DELETE_LABEL });
	}
}
