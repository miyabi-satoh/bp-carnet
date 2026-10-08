import { fetchPasswordPolicy, isNewPasswordValid, type PasswordPolicy } from '$lib/password';

/** 新しいパスワードと確認の2欄 (`new-password-fields.svelte`) を置く画面の状態。
 * ポリシーの取得と送信前の判定は画面によらず同じなので、ここに寄せる。 */
export class NewPasswordState {
	/** 取得できないままフォームを出すと、条件を知らせずに送信させることになるので、
	 * 取得できるまで (失敗したら `loadErrorMessage` を出して) フォームを出さない。 */
	policy = $state.raw<PasswordPolicy | null>(null);
	loadErrorMessage = $state('');
	password = $state('');
	confirmation = $state('');
	/** 入力中の欄にエラーを出し続けないよう、一度送信するまでは欄の下に何も出さない。 */
	submitted = $state(false);

	/** ポリシーを取得する。取得済みなら何もしない (ダイアログを開き直すたびに呼ばれるため)。 */
	loadPolicy = async () => {
		if (this.policy !== null) return;
		this.loadErrorMessage = '';
		const result = await fetchPasswordPolicy();
		if (result.ok) {
			this.policy = result.data;
		} else {
			this.loadErrorMessage = result.message;
		}
	};

	/** 入力を空に戻す。ダイアログを開き直すとき用。 */
	reset = () => {
		this.password = '';
		this.confirmation = '';
		this.submitted = false;
	};

	/** 送信時に呼ぶ。欄の下にエラーを出し始め、2欄とも問題が無ければ true を返す。 */
	validate = (): boolean => {
		if (this.policy === null) return false;
		this.submitted = true;
		return isNewPasswordValid(this.policy, this.password, this.confirmation);
	};
}
