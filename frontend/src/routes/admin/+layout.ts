import { redirect } from '@sveltejs/kit';
import { resolve } from '$app/paths';
import type { LayoutLoad } from './$types';

/** 管理者以外には、ユーザーメニューにも導線を出さない画面群 (docs/authentication.md)。ブックマークや
 * 履歴から直接開かれたときに、403 しか返らない画面を見せないようにする。 */
export const load: LayoutLoad = async ({ parent }) => {
	const { user } = await parent();
	if (user && user.role !== 'admin') redirect(307, resolve('/'));
};
