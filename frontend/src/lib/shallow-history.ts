// shallow routing (今のページの履歴に印を積む・置き換える `goto`) の呼び方を1か所にまとめる。
import { goto } from '$app/navigation';
import { page } from '$app/state';
import type { NavigationBase } from '@sveltejs/kit';

/** 印を積む・置き換える `goto` か。ページは移らないので、`beforeNavigate`・`afterNavigate` で移動の扱いをしない。 */
export function isShallowGoto({
	shallow,
	type
}: Pick<NavigationBase, 'shallow' | 'type'>): boolean {
	return shallow && type === 'goto';
}

/** 今のページの履歴に、`state` を持つ印を積む。 */
export function pushPageState(state: App.PageState): Promise<void> {
	// 今のページの履歴に印を積むだけで、別のページへ移らない。
	// eslint-disable-next-line svelte/no-navigation-without-resolve
	return goto('', { shallow: true, state });
}

/** 今のページの履歴を、`state` (省くと空) に置き換える。 */
export function replacePageState(state?: App.PageState): Promise<void> {
	// 今のページの履歴を置き換えるだけで、別のページへ移らない。
	// eslint-disable-next-line svelte/no-navigation-without-resolve
	return goto('', { shallow: true, replace: true, state });
}

/** 今の URL からクエリを取り除く。見せた知らせを、読み込み直しで出し直さないため。 */
export function clearQuery(): Promise<void> {
	// `page.url.pathname` は今のページの URL で、`resolve` 済み。
	// eslint-disable-next-line svelte/no-navigation-without-resolve
	return goto(page.url.pathname, { shallow: true, replace: true });
}
