/** 真になってから `delayMs` 続いたときだけ真になる値。速く終わる処理で読み込み中の表示が
 * 一瞬だけ光って消えるのを防ぐ。偽に戻ったら待たずに偽になる。
 *
 * 押せなくする (`disabled`) のは遅らせない。二重に押されるのを防ぐ役があるため。
 *
 * コンポーネントの初期化中に呼ぶ (`$effect` を使うため)。 */
export function delayedFlag(active: () => boolean, delayMs: number) {
	let visible = $state(false);

	$effect(() => {
		if (!active()) {
			visible = false;
			return;
		}
		const timer = setTimeout(() => (visible = true), delayMs);
		return () => clearTimeout(timer);
	});

	return {
		get current() {
			return visible;
		}
	};
}

/** 読み込み中の表示を出すまでの待ち時間。画面の中身を差し替えるものに使う。 */
export const LOADING_SHOW_DELAY_MS = 300;

/** 画面遷移のバーを出すまでの待ち時間。画面の端に細く出るだけなので、中身の差し替えより短くてよい。 */
export const NAVIGATION_SHOW_DELAY_MS = 150;
