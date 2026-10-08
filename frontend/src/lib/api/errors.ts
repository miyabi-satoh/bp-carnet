/**
 * backend のエラー envelope `{ error: { code, message } }` から `code` を取り出し、
 * 表示文言に変換する。`message` はローカライズされないデバッグ用の文字列なので表示しない。
 * 表示文言は Paraglide メッセージ (`messages/{locale}.json`) の `error_<code>` で持つ。
 */
import { findHelpTopic } from '$lib/help';
import * as m from '$lib/paraglide/messages.js';

/** backend の `AppError` が画面に返しうる `code`。Stripe の Webhook だけが受ける
 * `stripe_webhook_invalid_signature` は画面に届かないので持たない。 */
export const ERROR_CODES = [
	'not_found',
	'unauthorized',
	'invalid_credentials',
	'incorrect_password',
	'forbidden',
	'account_frozen',
	'app_update_required',
	'external_login_failed',
	'line_email_required',
	'google_email_untrusted',
	'external_account_conflict',
	'cannot_freeze_user',
	'cannot_delete_user',
	'cannot_cancel_deletion',
	'cannot_reset_password',
	'cannot_reset_own_password',
	'cannot_unlink_last_login_method',
	'username_taken',
	'too_many_requests',
	'invalid_email_token',
	'invalid_request_body',
	'invalid_path',
	'invalid_query',
	'csrf_rejected',
	'validation_error',
	'database_unavailable',
	'internal_error',
	'unknown_timezone',
	'ocr_budget_exhausted',
	'ocr_in_progress',
	'ocr_consent_required',
	'ocr_disabled',
	'ocr_image_too_large',
	'ocr_unsupported_mime_type',
	'ocr_upstream_error',
	'batch_validation_error',
	'record_conflict',
	'payments_disabled',
	'payments_upstream_error',
	'app_store_unavailable'
] as const;

export type ErrorCode = (typeof ERROR_CODES)[number];

const MESSAGES: Record<ErrorCode, () => string> = {
	not_found: m.error_not_found,
	unauthorized: m.error_unauthorized,
	invalid_credentials: m.error_invalid_credentials,
	incorrect_password: m.error_incorrect_password,
	forbidden: m.error_forbidden,
	// 問い合わせの一文は含めない。`errorMessage` が管理者の一文を足し、ログイン画面は
	// `contactlessMessage` で元の文を取って問い合わせ先を出し分ける。
	account_frozen: m.error_account_frozen,
	app_update_required: m.error_app_update_required,
	// 以下の4つはアプリの各社のログインだけが返す。ウェブは `?oauthError=` で受ける。
	external_login_failed: m.error_external_login_failed,
	line_email_required: m.error_line_email_required,
	google_email_untrusted: m.error_google_email_untrusted,
	external_account_conflict: m.error_external_account_conflict,
	cannot_freeze_user: m.error_cannot_freeze_user,
	cannot_delete_user: m.error_cannot_delete_user,
	cannot_cancel_deletion: m.error_cannot_cancel_deletion,
	cannot_reset_password: m.error_cannot_reset_password,
	cannot_reset_own_password: m.error_cannot_reset_own_password,
	cannot_unlink_last_login_method: m.error_cannot_unlink_last_login_method,
	username_taken: m.error_username_taken,
	too_many_requests: m.error_too_many_requests,
	invalid_email_token: m.error_invalid_email_token,
	invalid_request_body: m.error_invalid_request_body,
	invalid_path: m.error_invalid_path,
	invalid_query: m.error_invalid_query,
	csrf_rejected: m.error_csrf_rejected,
	validation_error: m.error_validation_error,
	database_unavailable: m.error_database_unavailable,
	internal_error: m.error_internal_error,
	unknown_timezone: m.error_unknown_timezone,
	ocr_budget_exhausted: m.error_ocr_budget_exhausted,
	ocr_in_progress: m.error_ocr_in_progress,
	ocr_consent_required: m.error_ocr_consent_required,
	ocr_disabled: m.error_ocr_disabled,
	ocr_image_too_large: m.error_ocr_image_too_large,
	ocr_unsupported_mime_type: m.error_ocr_unsupported_mime_type,
	ocr_upstream_error: m.error_ocr_upstream_error,
	batch_validation_error: m.error_batch_validation_error,
	record_conflict: m.error_record_conflict,
	payments_disabled: m.error_payments_disabled,
	payments_upstream_error: m.error_payments_upstream_error,
	app_store_unavailable: m.error_app_store_unavailable
};

/** 想定外の形のレスポンスや通信エラーに使う汎用文言。 */
export const GENERIC_ERROR_MESSAGE = m.error_generic;

/**
 * Google・LINE・Apple でのログインのコールバックはリダイレクトのみで完結し JSON envelope を経由しないため、
 * `?oauthError=` クエリパラメータとして渡ってくるコードは上の `ERROR_CODES` とは別に管理する。
 */
export const OAUTH_ERROR_CODES = [
	'google_failed',
	'google_disabled',
	'google_account_conflict',
	'google_email_untrusted',
	'line_failed',
	'line_disabled',
	'line_account_conflict',
	'line_email_required',
	'apple_failed',
	'apple_disabled',
	'apple_account_conflict',
	'account_frozen'
] as const;

export type OAuthErrorCode = (typeof OAUTH_ERROR_CODES)[number];

const OAUTH_MESSAGES: Record<OAuthErrorCode, () => string> = {
	google_failed: m.error_google_failed,
	google_disabled: m.error_google_disabled,
	google_account_conflict: m.error_google_account_conflict,
	google_email_untrusted: m.error_google_email_untrusted,
	line_failed: m.error_line_failed,
	line_disabled: m.error_line_disabled,
	line_account_conflict: m.error_line_account_conflict,
	line_email_required: m.error_line_email_required,
	apple_failed: m.error_apple_failed,
	apple_disabled: m.error_apple_disabled,
	apple_account_conflict: m.error_apple_account_conflict,
	// 凍結は ID/PW ログインと同じ文言。外部のログインのコールバックはボディを返せないため、
	// 同じ意味のコードをこちらにも持つ。
	account_frozen: m.error_account_frozen
};

/** 失敗の理由から、ダイアログでリンクする使い方の項目 (`help.ts` の slug)。内蔵ブラウザで開いて
 * いた場合の失敗 (`google_failed`・`line_failed`) と、LINE のメールアドレスの件だけ。設定の不備や
 * アカウントの衝突などは、使い方では直せないのでリンクしない。`?oauthError=` の値と API のエラーの
 * `code` を同じ表で引く (`NEEDS_CONTACT` と同じ)。 */
const HELP_SLUGS: Partial<Record<OAuthErrorCode | ErrorCode, string>> = {
	google_failed: 'trouble-in-app-browser',
	line_failed: 'trouble-in-app-browser',
	line_email_required: 'trouble-line-email'
};

/** 問い合わせ先を添えて知らせる失敗。本人では直せず、運営者 (管理者) に頼むしかないもの。
 * 文言には問い合わせの一文を含めず、ログイン画面はフォームへのリンクと合わせて出す (docs/authentication.md)。 */
const NEEDS_CONTACT: ReadonlySet<string> = new Set<OAuthErrorCode | ErrorCode>([
	'google_account_conflict',
	'line_account_conflict',
	'apple_account_conflict',
	'external_account_conflict',
	'account_frozen'
]);

/** `?oauthError=` の値か API のエラーの `code` が、問い合わせ先を添える失敗か。 */
export function needsContact(code: string | null | undefined): boolean {
	return code != null && NEEDS_CONTACT.has(code);
}

/** `?oauthError=` の値か API のエラーの `code` から、リンクする使い方の項目。無ければ `undefined`。
 * アプリで出さない項目 (`webOnly`) にもリンクしない。 */
export function errorHelpSlug(code: string | null | undefined): string | undefined {
	const slug =
		code != null && Object.hasOwn(HELP_SLUGS, code)
			? HELP_SLUGS[code as OAuthErrorCode | ErrorCode]
			: undefined;
	return slug !== undefined && findHelpTopic(slug) ? slug : undefined;
}

function isOAuthErrorCode(value: string): value is OAuthErrorCode {
	return (OAUTH_ERROR_CODES as readonly string[]).includes(value);
}

/** `?oauthError=` の値から表示文言を返す。`null` なら `undefined` (表示しない)。
 * 未知のコードは汎用文言に落とす。 */
export function oauthErrorMessage(code: string | null): string | undefined {
	if (code === null) return undefined;
	return isOAuthErrorCode(code) ? OAUTH_MESSAGES[code]() : GENERIC_ERROR_MESSAGE();
}

function isErrorCode(value: unknown): value is ErrorCode {
	return typeof value === 'string' && (ERROR_CODES as readonly string[]).includes(value);
}

/**
 * openapi-fetch の `error` (パース済み body) から `code` を取り出す。
 * 本文なしの 5xx では `undefined`、text/plain 応答では文字列になることがあるため、
 * 型に頼らず実行時に形を確認する。
 */
export function errorCode(body: unknown): ErrorCode | undefined {
	if (typeof body !== 'object' || body === null) return undefined;
	const error = (body as { error?: unknown }).error;
	if (typeof error !== 'object' || error === null) return undefined;
	const code = (error as { code?: unknown }).code;
	return isErrorCode(code) ? code : undefined;
}

/** `code` に対応する表示文言を返す。不明なら汎用文言。問い合わせ先を添える失敗には問い合わせの一文を足す。 */
export function errorMessage(body: unknown): string {
	const code = errorCode(body);
	if (!code) return GENERIC_ERROR_MESSAGE();
	return needsContact(code)
		? `${MESSAGES[code]()}${m.common_contact_operator()}`
		: MESSAGES[code]();
}

/** 問い合わせ先を添える失敗の、問い合わせの一文を含まない文言。問い合わせ先を出し分ける画面で使う。 */
export function contactlessMessage(code: ErrorCode): string {
	return MESSAGES[code]();
}

/** 呼び出しの失敗。`message` はそのまま画面に出せる文言。`code` は失敗の種類で呼び出し側の扱いを
 * 変えたいとき (例: 記録がほかで変わった・消された) に見る。通信エラーなど本文が無ければ `undefined`。 */
export type ApiFailure = { ok: false; message: string; code?: ErrorCode };

/** 呼び出しの結果。成功時に返す値があれば `T` に書く (例: `ApiResult<{ username: string }>`)。
 * `unwrap`/`ensureOk` と、それらを包む `$lib` のラッパーが同じ形を返すようにするための型。 */
export type ApiResult<T = object> = ({ ok: true } & T) | ApiFailure;

function failure(body: unknown): ApiFailure {
	return { ok: false, message: errorMessage(body), code: errorCode(body) };
}

/** openapi-fetch の呼び出し結果を、成功か「表示用の文言つきの失敗」に畳む。
 *
 * `if (!response.ok || !data)` を書いてから `errorMessage(error)` を引く、という同じ4行が
 * 呼び出しのたびに現れるため集約する。失敗時に何をするか (state に入れる・既定値を返す・
 * 画面遷移する) は呼び出し側で異なるので、判定と文言だけをここで扱う。
 *
 * `response.ok` でも `data` が無いケースを失敗に含めるのは、204 や本文の壊れた 200 を
 * 成功として扱うと、後続が undefined を掴んで別の場所で落ちるため。 */
export function unwrap<T>(result: {
	data?: T;
	error?: unknown;
	response: Response;
}): ApiResult<{ data: T }> {
	if (!result.response.ok || result.data === undefined) {
		return failure(result.error);
	}
	return { ok: true, data: result.data };
}

/** `unwrap()` の、応答ボディを使わない呼び出し向け。
 *
 * ログイン・記録の登録/更新/削除・ログアウトのように、成功の中身は見ずに成否だけを見る
 * 呼び出しがある。これらに `unwrap()` を使うと、204 や本文を返さない 200 を「データが無い」
 * として失敗に倒してしまうため、`response.ok` だけで判定するこちらを使う。 */
export function ensureOk(result: { error?: unknown; response: Response }): ApiResult {
	if (!result.response.ok) {
		return failure(result.error);
	}
	return { ok: true };
}

/** `unwrap()` を、openapi-fetch の呼び出し (Promise) ごと受け取る版。通信エラー (fetch の例外) も
 * 汎用文言の失敗に畳む。
 *
 * `$lib` のラッパーごとに `try { ... } catch { return 汎用文言 }` を書く定型を集約する。 */
export async function unwrapRequest<T>(
	request: Promise<{ data?: T; error?: unknown; response: Response }>
): Promise<ApiResult<{ data: T }>> {
	try {
		return unwrap(await request);
	} catch {
		return { ok: false, message: GENERIC_ERROR_MESSAGE() };
	}
}

/** `ensureOk()` を、呼び出し (Promise) ごと受け取る版。通信エラーの扱いは `unwrapRequest()` と同じ。 */
export async function ensureOkRequest(
	request: Promise<{ error?: unknown; response: Response }>
): Promise<ApiResult> {
	try {
		return ensureOk(await request);
	} catch {
		return { ok: false, message: GENERIC_ERROR_MESSAGE() };
	}
}
