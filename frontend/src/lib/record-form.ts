import { browser } from '$app/environment';
import { page } from '$app/state';
import { bpValueIssues, type BpField } from '$lib/bp-values';
import * as m from '$lib/paraglide/messages.js';
import { formatDateOnly, isLocalDateTime, localToday } from '$lib/period';
import { parseOptionalStrictInt } from '$lib/utils';

/** ホーム画面で記録フォームを開くよう頼まれているか。写真で記録のページの「数字で入力する」から
 * ホーム画面へ戻るときに使う。モジュールの変数で持つ (`$lib/file-handoff` と同じ)。 */
let manualEntryRequested = false;

/** ホーム画面へ移ったら、記録フォームを新規登録で開くよう頼む。 */
export function requestManualEntry(): void {
	manualEntryRequested = true;
}

/** `requestManualEntry` で頼まれていたかを返し、頼みを消す。 */
export function takeManualEntryRequest(): boolean {
	const requested = manualEntryRequested;
	manualEntryRequested = false;
	return requested;
}

/** 記録フォームの入力値。数値の欄は入力されたままの文字列で持つ (数字以外が入力されたことを
 * 「未入力」と区別してエラーにするため)。 */
export type RecordFormInput = {
	/** `<input type="date">` の値 ("YYYY-MM-DD")。 */
	measuredOnDate: string;
	/** `<input type="time">` の値 ("HH:MM")。 */
	time: string;
	systolic: string;
	diastolic: string;
	pulse: string;
};

/** 記録フォームの入力値一式 (メモを含む)。シートの初期値・現在値として扱う。 */
export type RecordFormValues = RecordFormInput & { memo: string };

/** 記録フォームの使われ方。見出しとボタンの文言はこれで決まる。
 * - `create`: 記録を追加する (ホームの「手動で入力...」・写真で記録)
 * - `edit`: 既存の記録や、取り込みの一覧の行を直す */
export type RecordFormMode = 'create' | 'edit';

/** 記録フォームの見出し。 */
export function recordFormTitle(mode: RecordFormMode): string {
	return mode === 'create' ? m.record_form_dialog_title() : m.record_form_dialog_edit_title();
}

/** 記録フォームの送信ボタンの文言。 */
export function recordFormSubmitLabel(mode: RecordFormMode): string {
	return mode === 'create'
		? m.record_form_dialog_create_button()
		: m.record_form_dialog_update_button();
}

/** 欄ごとのエラー文言。上下の血圧の大小関係のように2つの欄にまたがるエラーは、両方の欄に
 * 同じ文言が入る。 */
export type RecordFormErrors = Partial<Record<'measuredAt' | BpField, string>>;

export type RecordFormValidation =
	| {
			ok: true;
			/** API の `localMeasuredAt` に渡す形 ("YYYY-MM-DDTHH:MM")。 */
			measuredAtLocal: string;
			systolic: number;
			diastolic: number;
			pulse: number | undefined;
	  }
	| { ok: false; errors: RecordFormErrors };

/** 上下の血圧・脈拍の欄の入力値。 */
type BpFormInput = Pick<RecordFormInput, BpField>;

/** 上下の血圧・脈拍の欄ごとのエラー文言。 */
type BpFormErrors = Partial<Record<BpField, string>>;

type BpFormValidation =
	| { ok: true; systolic: number; diastolic: number; pulse: number | undefined }
	| { ok: false; errors: BpFormErrors };

/** `measuredOnDate` ("YYYY-MM-DD") ・`time` ("HH:MM") が実在する日時を表しているか。
 * 2つは連結してそのまま API の `localMeasuredAt` として送るため、連結した形で桁数まで確かめる。
 * 写真の読み取りは "7:30" のようにゼロ埋め無しの時刻や "25:99" のような実在しない時刻を返すことがある。 */
function isValidMeasuredAt(measuredOnDate: string, time: string): boolean {
	return isLocalDateTime(`${measuredOnDate}T${time}`);
}

/** API の `localMeasuredAt` ("YYYY-MM-DDTHH:MM") を、日時の2欄の値に分ける。分けた値は
 * `validateRecordForm` を通すと元の形に戻る。 */
export function splitMeasuredAt(localMeasuredAt: string): { measuredOnDate: string; time: string } {
	const [measuredOnDate = '', time = ''] = localMeasuredAt.split('T');
	return { measuredOnDate, time };
}

export function validateRecordForm(
	input: RecordFormInput,
	today = userToday()
): RecordFormValidation {
	const errors: RecordFormErrors = {};
	if (input.measuredOnDate === '' || input.time === '') {
		errors.measuredAt = m.record_error_required({ field: m.record_measured_at_label() });
	} else if (!isValidMeasuredAt(input.measuredOnDate, input.time)) {
		// 日時の欄は端末のピッカーから入るとは限らない (写真の読み取り・CSV の取り込み)。
		// ここで弾かないと、不正な日時のまま確認画面を通過し、取り込み全体がサーバーに拒否される (422)。
		errors.measuredAt = m.record_error_invalid({ field: m.record_measured_at_label() });
	} else if (today !== undefined && input.measuredOnDate > today) {
		// 年・月の打ち間違いで、普段の期間に出てこなくなるため。日付で比べ、今日の先の時刻は通す
		// (端末とサーバーの時計のずれで止めないため)。
		errors.measuredAt = m.record_error_future_date();
	}
	const bp = validateBpForm(input);
	if (!bp.ok) return { ok: false, errors: { ...errors, ...bp.errors } };
	if (errors.measuredAt !== undefined) return { ok: false, errors };
	return { ...bp, measuredAtLocal: `${input.measuredOnDate}T${input.time}` };
}

/** ログイン中のユーザーのタイムゾーンでの今日 (`YYYY-MM-DD`)。ログインしていない・ブラウザの外
 * (SSR しないので単体テストだけ) では `undefined` で、明日以降の日付の検査を省く。 */
function userToday(): string | undefined {
	if (!browser) return undefined;
	const timeZone: string | undefined = page.data.user?.timezone;
	return timeZone === undefined ? undefined : formatDateOnly(localToday(timeZone));
}

/** 上下の血圧・脈拍の欄を検証する (`validateRecordForm` の一部)。 */
export function validateBpForm(input: BpFormInput): BpFormValidation {
	const errors: BpFormErrors = {};
	const parse = (field: BpField, label: string, required: boolean): number | undefined => {
		const value = parseOptionalStrictInt(input[field]);
		if (value === undefined && required) {
			errors[field] = m.record_error_required({ field: label });
		} else if (value === null) {
			errors[field] = m.record_error_not_integer({ field: label });
		}
		return value ?? undefined;
	};
	const systolic = parse('systolic', m.record_systolic_label(), true);
	const diastolic = parse('diastolic', m.record_diastolic_label(), true);
	const pulse = parse('pulse', m.record_pulse_label(), false);

	for (const issue of bpValueIssues({ systolic, diastolic, pulse })) {
		for (const field of issue.fields) errors[field] ??= issue.message;
	}

	if (Object.keys(errors).length > 0 || systolic === undefined || diastolic === undefined) {
		return { ok: false, errors };
	}
	return { ok: true, systolic, diastolic, pulse };
}

/** 1行に1つの文言だけを出す取り込み画面 (エラーのある行・手書きメモ) 向けに、欄の並び順で
 * 最初のエラーを返す。エラーが無ければ `undefined`。 */
export function firstRecordFormError(input: RecordFormInput): string | undefined {
	const result = validateRecordForm(input);
	return result.ok ? undefined : firstFormError(result.errors);
}

/** 欄ごとのエラーのうち、欄の並び順で最初のもの。エラーが無ければ `undefined`。 */
function firstFormError(errors: RecordFormErrors): string | undefined {
	const { measuredAt, systolic, diastolic, pulse } = errors;
	return measuredAt ?? systolic ?? diastolic ?? pulse;
}
