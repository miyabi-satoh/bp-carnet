import { preloadCode } from '$app/navigation';

/** 画面で選んだファイルを、移った先のページへ渡す (ホーム → 写真で記録、設定 → 取り込み)。
 *
 * ADR: `File` は sessionStorage に置けないため、モジュールの変数で渡す (SPA で、画面の移動では
 * モジュールが読み直されない)。ページの読み込み直しで消えるので、受け取る側は「選ぶ」から始め直せるようにする。 */
export interface FileHandoff {
	/** 移る先のページのコードを先に読み込む。移るときにコードを取れない (サーバーが止まっている・起動中、
	 * デプロイで版が変わった) と、SvelteKit がページを読み込み直してファイルが消え、もう一度選ばせることに
	 * なるため。渡す側の画面を開いたときと、ファイルの選択を開いたときに呼ぶ。失敗しても、移るときに取り直す。 */
	preload(): void;
	set(file: File): void;
	/** `set` で渡したファイルを取り出して消す。無ければ `null`。 */
	take(): File | null;
}

/** `href` は移る先のページ (`resolve` 済み)。 */
export function createFileHandoff(href: string): FileHandoff {
	let pending: File | null = null;
	return {
		preload() {
			preloadCode(href).catch(() => {});
		},
		set(file) {
			pending = file;
		},
		take() {
			const file = pending;
			pending = null;
			return file;
		}
	};
}
