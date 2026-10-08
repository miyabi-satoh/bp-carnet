import { redirect } from '@sveltejs/kit';
import { resolve } from '$app/paths';
import type { PageLoad } from './$types';

/** パスワードを持たない Google 専用アカウントには、この画面で直せるものが無い
 * (設定画面にも行を出さない)。ブックマークや履歴から直接開かれたときに、必ず失敗する
 * フォームを見せないようにする。 */
export const load: PageLoad = async ({ parent }) => {
	const { user } = await parent();
	if (user && !user.passwordUsable) redirect(307, resolve('/settings'));
};
