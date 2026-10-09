// 利用規約・プライバシーポリシー (docs/authentication.md)。
// 本文は言語ごとの Markdown (`./legal/<言語>/<文書>.md`) に書き、mdsvex でコンポーネントにする (→ vite.config.ts)。
// 本文 (規約・ポリシーのどちらか) の表示される文言を変えたら、backend の規約の版 (`src/terms.rs` の `VERSION`) を上げる。
// リンクの書き方だけのように、読む人から見て同じなら上げない。

import type { Component } from 'svelte';
import { baseLocale, getLocale } from '$lib/paraglide/runtime.js';

// 料金のページは買い足し (有料化) 用の情報開示。規約・ポリシーと違い
// 同意の対象ではないので、変えても規約の版 (src/terms.rs の VERSION) は上げない。
export type LegalDocument = 'terms' | 'privacy' | 'pricing';

const sources = import.meta.glob<Component>('./legal/*/*.md', { import: 'default', eager: true });

/** `document` の本文を、表示言語 (無ければ日本語) で返す。 */
export function legalDocumentComponent(
	document: LegalDocument,
	locale: string = getLocale()
): Component {
	return (
		sources[`./legal/${locale}/${document}.md`] ?? sources[`./legal/${baseLocale}/${document}.md`]
	);
}
