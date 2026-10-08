// 使い方のページ (`/help`) の項目の定義。目次の並び・前後の項目は、ここだけから決まる。
// 項目ページの本文 (前置きと手順) は、言語ごとの `./help/<言語>/<slug>.md` に書く (→ HelpStep)。
import { createContext, type Component } from 'svelte';
import { isNativeApp } from '$lib/native-app';
import * as m from '$lib/paraglide/messages.js';
import { baseLocale, getLocale } from '$lib/paraglide/runtime.js';

export type HelpTopic = {
	/** URL (`/help/<slug>`)・本文のファイル名・画像のファイル名に使う。 */
	slug: string;
	title: () => string;
	/** ウェブ版 (ブラウザ) だけの話で、アプリでは出さない。アプリを入れる・ホーム画面に追加・内蔵ブラウザの案内は
	 * アプリでは要らず、Android など他のプラットフォームの名前も出てしまうため (App Review Guidelines 2.3.10)。 */
	webOnly?: true;
	/** アプリでは、本文を `<slug>.app.md` から読む。操作の手順がアプリとウェブで違う項目 (読み取りの買い足しは、
	 * ウェブは Stripe、アプリはアプリ内課金)。 */
	appBody?: true;
};

export type HelpSection = {
	title: () => string;
	/** まだ項目の無いまとまりは、目次に出さない。 */
	topics: readonly HelpTopic[];
};

const ALL_HELP_SECTIONS: readonly HelpSection[] = [
	{
		title: m.help_section_start_title,
		topics: [
			{
				slug: 'login',
				title: m.help_login_title
			},
			{
				slug: 'signup',
				title: m.help_signup_title
			}
		]
	},
	{
		title: m.help_section_record_title,
		topics: [
			{
				slug: 'record-by-hand',
				title: m.help_record_by_hand_title
			},
			{
				slug: 'record-by-photo',
				title: m.help_record_by_photo_title
			},
			{
				slug: 'import-notebook',
				title: m.help_import_notebook_title
			},
			{
				slug: 'edit-record',
				title: m.help_edit_record_title
			}
		]
	},
	{
		title: m.help_section_view_title,
		topics: [
			{
				slug: 'view-records',
				title: m.help_view_records_title
			}
		]
	},
	{
		title: m.help_section_print_title,
		topics: [
			{
				slug: 'print-report',
				title: m.help_print_report_title
			}
		]
	},
	{
		title: m.help_section_settings_title,
		topics: [
			{
				slug: 'settings-period',
				title: m.help_settings_period_title
			},
			{
				slug: 'settings-timezone',
				title: m.help_settings_timezone_title
			},
			{
				slug: 'theme',
				title: m.help_theme_title
			},
			{
				slug: 'export-csv',
				title: m.help_export_csv_title
			},
			{
				slug: 'import-csv',
				title: m.help_import_csv_title
			},
			{
				slug: 'delete-account',
				title: m.help_delete_account_title
			}
		]
	},
	{
		title: m.help_section_trouble_title,
		topics: [
			{
				slug: 'trouble-login',
				title: m.help_trouble_login_title
			},
			{
				slug: 'trouble-in-app-browser',
				title: m.help_trouble_in_app_browser_title,
				webOnly: true
			},
			{
				slug: 'trouble-line-email',
				title: m.help_trouble_line_email_title
			},
			{
				slug: 'trouble-photo',
				title: m.help_trouble_photo_title
			},
			{
				slug: 'trouble-ocr-budget',
				title: m.help_trouble_ocr_budget_title,
				appBody: true
			},
			{
				slug: 'iphone-app',
				title: m.help_iphone_app_title,
				webOnly: true
			},
			{
				slug: 'home-screen-android',
				title: m.help_home_screen_android_title,
				webOnly: true
			}
		]
	}
];

/** 目次の並び。前後の項目もこの順で決まる。アプリでは `webOnly` の項目を除く。 */
export const HELP_SECTIONS: readonly HelpSection[] = isNativeApp
	? ALL_HELP_SECTIONS.map((section) => ({
			...section,
			topics: section.topics.filter((topic) => !topic.webOnly)
		}))
	: ALL_HELP_SECTIONS;

/** 目次の順に並べたすべての項目。 */
export const HELP_TOPICS: readonly HelpTopic[] = HELP_SECTIONS.flatMap((section) => section.topics);

export function findHelpTopic(slug: string): HelpTopic | undefined {
	return HELP_TOPICS.find((topic) => topic.slug === slug);
}

/** 目次の順で、`topic` の前と後の項目。端では `undefined`。 */
export function adjacentHelpTopics(topic: HelpTopic): {
	prev: HelpTopic | undefined;
	next: HelpTopic | undefined;
} {
	const index = HELP_TOPICS.indexOf(topic);
	return { prev: HELP_TOPICS[index - 1], next: HELP_TOPICS[index + 1] };
}

/**
 * 手順の画像の `static/` からのパス。`n` は手順の番号 (1 始まり)。
 * 画像は E2E で撮る (`just help-shots`、→ e2e/help-shots.ts)。
 */
export function helpStepImagePath(slug: string, n: number): `/help/${string}` {
	return `/help/${slug}-${n}.webp`;
}

/** 手順の画像の大きさ (px)。スマホの幅 (Pixel 7) の画面を縮めて撮る (→ e2e/help-shots.ts)。 */
export const HELP_STEP_IMAGE_SIZE = { width: 720, height: 1465 } as const;

// 本文は項目ページを開いたときに読み込む (目次やエラーの案内から `$lib/help` を読むだけでは読み込まない)。
const contents = import.meta.glob<Component>('./help/*/*.md', { import: 'default' });

/** `topic` の本文 (前置きと手順) を、表示言語 (無ければ日本語) で読み込む。アプリでは `appBody` の項目をアプリの本文にする。 */
export function loadHelpTopicContent(
	topic: HelpTopic,
	locale: string = getLocale(),
	app: boolean = isNativeApp
): Promise<Component> {
	const name = app && topic.appBody ? `${topic.slug}.app` : topic.slug;
	const load =
		contents[`./help/${locale}/${name}.md`] ?? contents[`./help/${baseLocale}/${name}.md`];
	return load();
}

/** 本文の手順 (HelpStep) に、開いている項目の slug を渡す。項目を移っても同じページのまま描き直すので、値でなく関数で渡す。 */
export const [getHelpTopicSlug, setHelpTopicSlug] = createContext<() => string>();
