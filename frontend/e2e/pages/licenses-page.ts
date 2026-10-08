import type { Locator, Page } from '@playwright/test';
import { ja } from '../helpers';

/** 第三者のソフトウェア (`/licenses`) のページ。 */
export class LicensesPage {
	constructor(private readonly page: Page) {}

	async open(): Promise<void> {
		await this.page.goto('/licenses');
	}

	get heading(): Locator {
		return this.page.getByRole('heading', { level: 1 });
	}

	/** 節 (「画面」「iPhone アプリ」)。 */
	section(name: string): Locator {
		return this.page.getByRole('region', { name, exact: true });
	}

	get webSection(): Locator {
		return this.section(ja.licenses_section_web);
	}

	get iosSection(): Locator {
		return this.section(ja.licenses_section_ios);
	}

	/** 節の中の、その名前のパッケージの行。 */
	packageItem(section: Locator, name: string): Locator {
		return section
			.getByRole('listitem')
			.filter({ has: this.page.getByText(name, { exact: true }) });
	}

	/** 行を開いて、ソースの置き場所と条文を出す。開閉のつまみ (`summary`) には役割が無いので、要素の名前で探す。 */
	async openPackage(item: Locator): Promise<void> {
		await item.locator('summary').click();
	}

	/** 開いた行のソースの置き場所のリンク。 */
	sourceLink(item: Locator): Locator {
		return item.getByRole('link');
	}
}
