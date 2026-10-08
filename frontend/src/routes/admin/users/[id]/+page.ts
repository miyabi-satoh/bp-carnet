import { error } from '@sveltejs/kit';
import type { PageLoad } from './$types';

/** URL の id が正の整数でなければ、API に問い合わせる前に 404 にする。数値として読めない id を
 * そのまま送ると、入力の誤りを伝えるエラーが出て画面と噛み合わないため。 */
export const load: PageLoad = ({ params }) => {
	if (!/^[1-9]\d*$/.test(params.id)) error(404);
};
