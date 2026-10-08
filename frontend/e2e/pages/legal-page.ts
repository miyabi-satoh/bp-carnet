import type { Locator, Page } from '@playwright/test';
import { ja } from '../helpers';

/** 利用規約 (`/terms`)・プライバシーポリシー (`/privacy`) のページ。 */
export class LegalPage {
	constructor(private readonly page: Page) {}

	get heading(): Locator {
		return this.page.getByRole('heading', { level: 1 });
	}

	get backButton(): Locator {
		return this.page.getByRole('button', { name: ja.common_back_button, exact: true });
	}

	/** 本文の節の見出し。 */
	sectionHeading(name: string): Locator {
		return this.page.getByRole('heading', { level: 2, name, exact: true });
	}

	/** 本文。 */
	get body(): Locator {
		return this.page.getByRole('article');
	}

	/** 本文の中のリンク。 */
	link(name: string): Locator {
		return this.body.getByRole('link', { name, exact: true });
	}

	/** 本文の表。 */
	get tables(): Locator {
		return this.body.getByRole('table');
	}
}
