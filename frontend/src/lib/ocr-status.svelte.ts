import { LatestRequest } from '$lib/latest-request';
import { fetchOcrStatus, type OcrStatus } from '$lib/ocr';

/** 取得する前の値。取得できなかったとき (`fetchOcrStatus`) と同じく、分からないものは出さない側にそろえる。 */
const UNKNOWN_OCR_STATUS: OcrStatus = {
	enabled: null,
	quotas: null,
	quotaLow: false,
	topupAvailable: false,
	consented: null
};

/** 写真の読み取りの状態 (枠の残り・買い足せるか・同意) を出す画面 (写真で記録・設定) の状態。
 * 読み取りや買い足しの後に取り直すので、遅れて返った古い応答で新しい値を上書きしない。 */
export class OcrStatusState {
	status = $state<OcrStatus>({ ...UNKNOWN_OCR_STATUS });

	#request = new LatestRequest();
	#keepKnownConsent: boolean;

	/** `keepKnownConsent` を付けると、取り直しに失敗して同意が分からなくなっても、前に分かった同意を残す
	 * (同意したことを忘れて、読み取る前に聞き直さないため)。 */
	constructor({ keepKnownConsent = false }: { keepKnownConsent?: boolean } = {}) {
		this.#keepKnownConsent = keepKnownConsent;
	}

	/** 取り直す。古くなって捨てたときは `false` を返す。 */
	async refresh(): Promise<boolean> {
		const isLatest = this.#request.begin();
		const status = await fetchOcrStatus();
		if (!isLatest()) return false;
		const consented =
			this.#keepKnownConsent && status.consented === null
				? this.status.consented
				: status.consented;
		this.status = { ...status, consented };
		return true;
	}

	/** 画面の操作で同意が変わったとき (同意した・取り消した・ほかの端末で取り消していた)。 */
	setConsented(consented: boolean | null) {
		this.status.consented = consented;
	}

	/** 取得中の応答を捨てる (画面を離れたとき)。 */
	cancel() {
		this.#request.cancel();
	}
}
