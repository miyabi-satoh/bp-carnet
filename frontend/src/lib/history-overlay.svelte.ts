import { pushState } from '$app/navigation';
import { page } from '$app/state';

/** `PageState` のうち、開いた印 (`true`) を置く鍵。 */
type OverlayStateKey = {
	[K in keyof App.PageState]-?: App.PageState[K] extends true | undefined ? K : never;
}[keyof App.PageState];

/** 開くと `pushState` で履歴に印を積み、端末の戻る操作でその印から出ると閉じる、ページの上に重ねるもの
 * (記録フォームのシート・写真の拡大)。コンポーネントの script で1つ作る (中で `$effect` を張る)。
 *
 * 開いているかは印から決める (`$derived`) のではなく自分で持つ。閉じた後の「進む」で開き直さないため。 */
export class HistoryOverlay {
	open = $state(false);

	#key: OverlayStateKey;
	/** 自分で閉じて、開いたときに積んだ履歴を戻している途中か (戻る操作が届くまで)。$state にしないのは、
	 * effect の中で読むだけで、変わったことで effect を走らせる必要が無いため。 */
	#leaving = false;
	/** 一度でも開いたか。開いた後に「進む」で印の残った履歴に入ったときだけ戻る。作られた時点で印が
	 * 残っているのは、開いたまま別のページへ移ってブラウザの戻る操作で戻ってきたときや、印の付いた履歴へ
	 * 戻ってから開き直したときなどで、そこで戻ると戻ってきたページを飛ばしてさらに前へ行ってしまう。 */
	#openedHere = false;

	/** `onBackWhileOpen` は、開いたまま戻る操作で印から出たときに呼ぶ。閉じるなら `true` を返す。
	 * `false` なら印を積み直して開いたままにする。渡さなければいつも閉じる。 */
	constructor(key: OverlayStateKey, onBackWhileOpen: () => boolean = () => true) {
		this.#key = key;
		$effect(() => {
			if (this.marked) {
				// 閉じた後の「進む」で、印の残った履歴に入った。開き直さず、その履歴から戻る。
				// 留まると、画面は同じなのに履歴が1つ余分に残り、次の「戻る」が空振りする。
				// `close()` が自分で戻している間 (`open` を落としてから戻る操作が届くまで) は、二重に戻らない。
				if (!this.open && !this.#leaving && this.#openedHere) history.back();
				return;
			}
			if (!this.open) {
				this.#leaving = false;
				return;
			}
			// 端末の戻る操作で印の履歴から出た (`back()` もここを通る)。
			if (onBackWhileOpen()) this.open = false;
			else this.#keepOpen();
		});
	}

	/** 今の履歴に印が付いているか。 */
	get marked(): boolean {
		return page.state[this.#key] === true;
	}

	/** 印を積んで開く。戻る・進む操作で印の残った履歴に入っていても、閉じたときに前の画面へ戻らないよう必ず積む。 */
	show() {
		this.#keepOpen();
		this.open = true;
		this.#openedHere = true;
	}

	#keepOpen() {
		pushState('', { ...page.state, [this.#key]: true });
	}

	/** すぐ閉じ、開いたときに積んだ印を戻す。 */
	close() {
		this.open = false;
		if (this.marked) {
			this.#leaving = true;
			history.back();
		}
	}

	/** 印を戻して閉じる。閉じるのは戻る操作が届いてから (`open` はそれまで `true` のまま)。 */
	back() {
		history.back();
	}
}
