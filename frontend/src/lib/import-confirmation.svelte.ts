import { importRows, type ExistingEntry, type ImportOutcome, type ImportRow } from '$lib/import';
import { LatestRequest } from '$lib/latest-request';
import * as m from '$lib/paraglide/messages.js';

/** 確認画面 (`ImportPreview`) で確定した取り込みの実行と、その間の状態 (CSV の取り込みのページ・写真で
 * 記録のページで共通)。ページの script で1つ作り、破棄するときに `reset` する。
 *
 * ADR: 取り込みの実行は確認画面ではなく、呼び出し側のページで持つ。送信中は見出しの戻るボタンと
 * ページを離れる操作 (`LeaveGuard`) も止めるため、確認画面の外からも送信中かを読む。 */
export class ImportConfirmation {
	/** 取り込み中か。この間は確定ボタンを押せず、ページも離れない。 */
	importing = $state(false);
	/** 取り込みに失敗したときの文言。 */
	error = $state('');
	/** 確認画面を出してから記録がほかで変わり、出し直したときの案内 (docs/import-export.md)。 */
	notice = $state('');
	/** 確認画面の既存の記録を取り直すたびに増やす (`ImportPreview` の `reloadKey`)。 */
	reloadKey = $state(0);

	#importRequest = new LatestRequest();
	#onImported: (created: number) => void;

	constructor(onImported: (created: number) => void) {
		this.#onImported = onImported;
	}

	/** 取り込みを実行する。成功したら、登録した件数を渡して `onImported` を呼ぶ (閉じ終わるまで `importing` のままにする)。
	 * `existing` は確認画面に出した既存の記録。 */
	async confirm(rows: ImportRow[], existing: ExistingEntry[]): Promise<void> {
		if (this.importing) return;
		const isLatest = this.#importRequest.begin();
		this.importing = true;
		this.error = '';
		this.notice = '';
		let outcome: ImportOutcome;
		try {
			outcome = await importRows(rows, existing);
		} catch (error) {
			// 送信中の表示のまま固まるとダイアログごと閉じられなくなるため、想定外の例外でも
			// 必ず解除する。握り潰さず投げ直すのは、呼び出し順序の誤り (`importRows` は対象行が
			// 無いと throw する) を通信エラーに見せないため。
			if (isLatest()) this.importing = false;
			throw error;
		}
		if (!isLatest()) return;
		if (outcome.ok) {
			this.#onImported(outcome.created);
			return;
		}
		this.importing = false;
		if (outcome.code === 'record_conflict') {
			// 取り込まずに止め、確認画面を最新の記録で出し直す。
			this.notice = m.import_preview_changed_elsewhere();
			this.reloadKey += 1;
			return;
		}
		this.error = outcome.message;
	}

	/** 閉じた・破棄したとき。進行中の取り込みの結果は捨てる。 */
	reset() {
		this.#importRequest.cancel();
		this.importing = false;
		this.error = '';
		this.notice = '';
	}
}
