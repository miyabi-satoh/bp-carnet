import { beforeNavigate, goto } from '$app/navigation';
import type { BeforeNavigate } from '@sveltejs/kit';

/** 入力の途中でページを離れる前に確認する。コンポーネントの script で1つ作り、
 * `LeaveConfirmDialog` に渡す。
 *
 * - アプリの中の移動 (戻る操作・リンク・`goto`) は止めて、アプリのダイアログで聞く。
 * - 再読み込み・タブを閉じるときは、ブラウザ標準の確認に任せる (SvelteKit は `leave` を止めると出す)。
 * - `pushState` で積んだ履歴の行き来は `beforeNavigate` を通らないので、止めない。 */
export class LeaveGuard {
	/** 確認のダイアログを開いているか。 */
	dialogOpen = $state(false);

	#isDirty: () => boolean;
	#isBusy: () => boolean;
	/** 確認なしで離れてよくなったか (確認で「離れる」を選んだ・取り込みが済んだ)。 */
	#allowed = false;
	#pending: BeforeNavigate | null = null;
	/** `confirm` で止めた、ページの中で入力を捨てる操作。 */
	#pendingAction: (() => void) | null = null;

	/** `isBusy` の間は、確認も出さずに移動を止める (送信中に離れると、結果を伝えられないうえ、
	 * 離れた先で始めた次の操作を、後から終わった送信が上書きしうるため)。 */
	constructor(isDirty: () => boolean, isBusy: () => boolean = () => false) {
		this.#isDirty = isDirty;
		this.#isBusy = isBusy;
		beforeNavigate((navigation) => {
			if (this.#allowed) return;
			if (this.#isBusy()) {
				navigation.cancel();
				return;
			}
			if (!this.#isDirty()) return;
			navigation.cancel();
			if (navigation.type === 'leave') return;
			this.#pending = navigation;
			this.dialogOpen = true;
		});
	}

	/** 以降は確認せずに離れられるようにする。 */
	allow() {
		this.#allowed = true;
	}

	/** ページの中で入力を捨てる操作 (シートを閉じるなど) の前に確認する。直していなければすぐ `action` を呼び、
	 * 送信中なら何もしない。 */
	confirm(action: () => void) {
		if (this.#isBusy()) return;
		if (!this.#isDirty()) {
			action();
			return;
		}
		this.#pendingAction = action;
		this.dialogOpen = true;
	}

	/** 確認で「離れる」を選んだとき。止めた移動・操作をやり直す。 */
	leave() {
		const action = this.#pendingAction;
		const navigation = this.#pending;
		this.#pendingAction = null;
		this.#pending = null;
		this.dialogOpen = false;
		if (action) {
			action();
			return;
		}
		if (!navigation?.to) return;
		this.#allowed = true;
		if (navigation.type === 'popstate' && navigation.delta !== undefined) {
			// 止めた戻る操作は、SvelteKit が元の履歴へ進め直している。同じだけ動かしてやり直す。
			history.go(navigation.delta);
		} else {
			// `to.url` は移動先として解決済みの URL。
			// eslint-disable-next-line svelte/no-navigation-without-resolve
			goto(navigation.to.url);
		}
	}

	/** 確認でとどまったとき (ダイアログを閉じたときも)。 */
	stay() {
		this.#pendingAction = null;
		this.#pending = null;
		this.dialogOpen = false;
	}
}
