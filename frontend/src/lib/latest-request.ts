/** 非同期処理の結果を、最後に始めた処理だけが反映できるようにする。 */
export class LatestRequest {
	#generation = 0;

	/** 新しく始める。返す関数は、その処理がまだ最新かを返す。 */
	begin(): () => boolean {
		const generation = ++this.#generation;
		return () => generation === this.#generation;
	}

	/** 進行中の結果をすべて捨てる (破棄・閉じたとき)。 */
	cancel(): void {
		this.#generation += 1;
	}
}
