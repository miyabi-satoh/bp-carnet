import type { Locator, Page } from '@playwright/test';
import { ja } from '../helpers';
import { HelpTopicPage } from './help-topic-page';

/** 使い方の目次 (`/help`)。ログインなしで読める。 */
export class HelpIndexPage {
	constructor(private readonly page: Page) {}

	async open(): Promise<void> {
		await this.page.goto('/help');
	}

	get heading(): Locator {
		return this.page.getByRole('heading', { name: ja.help_title, exact: true });
	}

	/** 目次から `title` (`ja.help_login_title` など) の項目へ移る。`slug` は移った先の URL の末尾。 */
	async openTopic(title: string, slug: string): Promise<HelpTopicPage> {
		await this.page.getByRole('link', { name: title, exact: true }).click();
		await this.page.waitForURL(`/help/${slug}`);
		return new HelpTopicPage(this.page);
	}
}
