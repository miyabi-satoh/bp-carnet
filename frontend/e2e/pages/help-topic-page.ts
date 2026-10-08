import type { Locator, Page } from '@playwright/test';
import { ja } from '../helpers';

/** 使い方の項目 (`/help/[slug]`)。手順の並びと、前後の項目・目次へのリンクがある。 */
export class HelpTopicPage {
	constructor(private readonly page: Page) {}

	/** `slug` の項目を直接開く。 */
	async open(slug: string): Promise<void> {
		await this.page.goto(`/help/${slug}`);
	}

	heading(title: string): Locator {
		return this.page.getByRole('heading', { name: title, exact: true });
	}

	/** 手順の並び。 */
	get steps(): Locator {
		return this.page.getByRole('listitem');
	}

	get stepImages(): Locator {
		return this.steps.getByRole('img');
	}

	get stepLinks(): Locator {
		return this.steps.getByRole('link');
	}

	/** 手順の中の、新しいタブで開く外のサイトへのリンク。 */
	externalLink(label: string): Locator {
		return this.steps.getByRole('link', { name: `${label} ${ja.common_opens_in_new_tab}` });
	}

	get prevLink(): Locator {
		return this.page.getByRole('link', { name: ja.help_prev_topic_label });
	}

	get nextLink(): Locator {
		return this.page.getByRole('link', { name: ja.help_next_topic_label });
	}

	get backToIndexLink(): Locator {
		return this.page.getByRole('link', { name: ja.help_back_to_index_button, exact: true });
	}

	/** 見出しの戻るボタン。 */
	get backLink(): Locator {
		return this.page.getByRole('link', { name: ja.common_back_button, exact: true });
	}
}
