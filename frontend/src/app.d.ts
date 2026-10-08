// See https://svelte.dev/docs/kit/types#app.d.ts
// for information about these interfaces
/// <reference types="vite-plugin-pwa/client" />
declare global {
	namespace App {
		// interface Error {}
		// interface Locals {}
		// interface PageData {}
		interface PageState {
			/** 取り込みのページ (`/settings/import`)・写真で記録のページ (`/record/photo`) で、
			 * 取り込み内容の確認の段階へ進んだ履歴か。 */
			importStep?: 'preview';
			/** 記録フォームのシート (`record-form-sheet.svelte`) を開いた履歴か。戻る操作でシートを閉じる。 */
			recordForm?: true;
			/** 写真の拡大表示 (`photo-lightbox.svelte`) を開いた履歴か。戻る操作で閉じる。 */
			photoLightbox?: true;
		}
		// interface Platform {}
	}
}

export {};
