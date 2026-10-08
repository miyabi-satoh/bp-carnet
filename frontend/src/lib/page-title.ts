// 画面の <title>。索引させる公開ページ (src/seo.rs の表) はサーバーが素の HTML に差し込むのと同じ文言を
// ここでも出す。JS を実行する検索エンジンが見る描画後の DOM でも、ページ別の title を効かせるため。
// 文言の食い違いは Rust 側のテスト (`page_titles_match_the_frontend`) が拾う。
import { findHelpTopic } from '$lib/help';
import * as m from '$lib/paraglide/messages.js';

export function pageTitle(routeId: string | null, slug?: string): string {
	const app = m.app_name();
	switch (routeId) {
		// 未ログインの `/` はログイン画面へ遷移して見えるため、`/login` も同じ title にする。
		case '/':
		case '/login':
			return m.seo_top_title();
		case '/terms':
			return `${m.terms_title()} — ${app}`;
		case '/privacy':
			return `${m.privacy_title()} — ${app}`;
		case '/licenses':
			return `${m.licenses_title()} — ${app}`;
		case '/help':
			return `${m.help_title()} — ${app}`;
		case '/help/[slug]': {
			const topic = slug ? findHelpTopic(slug) : undefined;
			return topic ? `${topic.title()} — ${m.help_title()} — ${app}` : app;
		}
		default:
			return app;
	}
}
