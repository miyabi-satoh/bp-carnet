import { describe, expect, it } from 'vitest';
import * as m from '$lib/paraglide/messages.js';
import {
	GENERIC_ERROR_MESSAGE,
	ensureOk,
	ensureOkRequest,
	errorCode,
	errorMessage,
	needsContact,
	errorHelpSlug,
	unwrap,
	unwrapRequest
} from './errors';

describe('errorCode', () => {
	it('extracts a known code from the envelope', () => {
		expect(errorCode({ error: { code: 'invalid_credentials', message: 'x' } })).toBe(
			'invalid_credentials'
		);
	});

	it('returns undefined for unknown codes and malformed bodies', () => {
		expect(errorCode({ error: { code: 'something_else' } })).toBeUndefined();
		expect(errorCode({ message: 'legacy shape' })).toBeUndefined();
		expect(errorCode('plain text')).toBeUndefined();
		expect(errorCode(undefined)).toBeUndefined();
		expect(errorCode(null)).toBeUndefined();
	});
});

describe('errorMessage', () => {
	it('maps a known code to a Japanese message', () => {
		expect(errorMessage({ error: { code: 'too_many_requests', message: 'x' } })).toContain('上限');
	});

	it('凍結は、ログイン画面の外では運営者への問い合わせを添える', () => {
		expect(errorMessage({ error: { code: 'account_frozen', message: 'x' } })).toBe(
			`${m.error_account_frozen()}${m.common_contact_operator()}`
		);
	});

	it('アプリの各社のログインだけが返すコードも、それぞれの文言にする', () => {
		expect(errorMessage({ error: { code: 'external_account_conflict', message: 'x' } })).toBe(
			`${m.error_external_account_conflict()}${m.common_contact_operator()}`
		);
		expect(errorMessage({ error: { code: 'line_email_required', message: 'x' } })).toBe(
			m.error_line_email_required()
		);
		expect(errorMessage({ error: { code: 'external_login_failed', message: 'x' } })).toBe(
			m.error_external_login_failed()
		);
	});

	it('maps invalid_request_body (malformed JSON / missing field / wrong content-type)', () => {
		expect(errorMessage({ error: { code: 'invalid_request_body', message: 'x' } })).toContain(
			'入力内容'
		);
	});

	it('maps validation_error (value out of range etc.)', () => {
		expect(errorMessage({ error: { code: 'validation_error', message: 'x' } })).toContain(
			'入力内容'
		);
	});

	it('maps ocr_disabled (GEMINI_API_KEY not configured)', () => {
		expect(errorMessage({ error: { code: 'ocr_disabled', message: 'x' } })).toContain(
			'現在利用できません'
		);
	});

	it('maps ocr_unsupported_mime_type', () => {
		expect(errorMessage({ error: { code: 'ocr_unsupported_mime_type', message: 'x' } })).toContain(
			'対応していない画像形式'
		);
	});

	it('maps ocr_upstream_error', () => {
		expect(errorMessage({ error: { code: 'ocr_upstream_error', message: 'x' } })).toBe(
			m.error_ocr_upstream_error()
		);
	});

	it('falls back to the generic message', () => {
		expect(errorMessage(undefined)).toBe(GENERIC_ERROR_MESSAGE());
	});
});

describe('unwrap', () => {
	const ok = (body?: BodyInit | null) => new Response(body ?? null, { status: 200 });
	const ng = (status: number) => new Response(null, { status });

	it('returns the data on success', () => {
		const result = unwrap({ data: { id: 1 }, response: ok() });
		expect(result).toEqual({ ok: true, data: { id: 1 } });
	});

	it('turns an error envelope into its display message', () => {
		const result = unwrap({
			error: { error: { code: 'not_found', message: 'x' } },
			response: ng(404)
		});
		expect(result.ok).toBe(false);
		expect(result).toMatchObject({
			message: errorMessage({ error: { code: 'not_found' } }),
			code: 'not_found'
		});
	});

	// 204 や本文の壊れた 200 を成功として扱うと、後続が undefined を掴んで別の場所で落ちる。
	it('treats a successful response without data as a failure', () => {
		const result = unwrap({ response: ok() });
		expect(result).toEqual({ ok: false, message: GENERIC_ERROR_MESSAGE() });
	});

	it('falls back to the generic message for an unknown error shape', () => {
		const result = unwrap({ error: 'not an envelope', response: ng(500) });
		expect(result).toEqual({ ok: false, message: GENERIC_ERROR_MESSAGE() });
	});

	// エラー応答が本文を伴う場合 (バリデーションエラーの envelope 等) でも、成功判定は
	// あくまで response.ok が先。data の有無で成功に転ばないことを固定する。
	it('treats a failed response as a failure even when it carries data', () => {
		const result = unwrap({
			data: { id: 1 },
			error: { error: { code: 'validation_error', message: 'x' } },
			response: ng(422)
		});
		expect(result).toEqual({
			ok: false,
			message: errorMessage({ error: { code: 'validation_error' } }),
			code: 'validation_error'
		});
	});

	// null は「値がある」として扱う。API が明示的に null を返す場合と、本文が無い場合を
	// 区別できないと、前者を誤ってエラーにしてしまう。
	it('treats a null payload as data', () => {
		expect(unwrap({ data: null, response: ok() })).toEqual({ ok: true, data: null });
	});
});

describe('ensureOk', () => {
	// ensureOk は本文を見ないため、成功・失敗ともステータスだけを変えれば足りる。
	const res = (status: number) => new Response(null, { status });

	// `unwrap()` との違いはここ。ログアウト (204) や本文を返さない 200 は成功として扱う。
	it('treats a successful response without a body as a success', () => {
		expect(ensureOk({ response: res(204) })).toEqual({ ok: true });
		expect(ensureOk({ response: res(200) })).toEqual({ ok: true });
	});

	it('turns an error envelope into its display message', () => {
		const result = ensureOk({
			error: { error: { code: 'invalid_credentials', message: 'x' } },
			response: res(401)
		});
		expect(result).toEqual({
			ok: false,
			message: errorMessage({ error: { code: 'invalid_credentials' } }),
			code: 'invalid_credentials'
		});
	});

	it('falls back to the generic message for an unknown error shape', () => {
		expect(ensureOk({ error: 'not an envelope', response: res(500) })).toEqual({
			ok: false,
			message: GENERIC_ERROR_MESSAGE()
		});
	});
});

describe('unwrapRequest', () => {
	it('unwraps the resolved result', async () => {
		const request = Promise.resolve({ data: { id: 1 }, response: new Response(null) });
		expect(await unwrapRequest(request)).toEqual({ ok: true, data: { id: 1 } });
	});

	// fetch 自体の失敗 (オフライン等) は例外になるため、呼び出し側に try/catch を書かせない。
	it('turns a network error into the generic message', async () => {
		const request = Promise.reject(new TypeError('Failed to fetch'));
		expect(await unwrapRequest(request)).toEqual({ ok: false, message: GENERIC_ERROR_MESSAGE() });
	});
});

describe('ensureOkRequest', () => {
	it('checks the resolved result', async () => {
		const request = Promise.resolve({ response: new Response(null, { status: 204 }) });
		expect(await ensureOkRequest(request)).toEqual({ ok: true });
	});

	it('turns a network error into the generic message', async () => {
		const request = Promise.reject(new TypeError('Failed to fetch'));
		expect(await ensureOkRequest(request)).toEqual({
			ok: false,
			message: GENERIC_ERROR_MESSAGE()
		});
	});
});

describe('errorHelpSlug', () => {
	it('内蔵ブラウザで起きうる失敗と LINE のメールアドレスの件は、使い方の項目を返す', () => {
		expect(errorHelpSlug('google_failed')).toBe('trouble-in-app-browser');
		expect(errorHelpSlug('line_failed')).toBe('trouble-in-app-browser');
		expect(errorHelpSlug('line_email_required')).toBe('trouble-line-email');
	});

	it('使い方では直せない失敗・未知のコード・指定なしは返さない', () => {
		expect(errorHelpSlug('google_account_conflict')).toBeUndefined();
		expect(errorHelpSlug('account_frozen')).toBeUndefined();
		expect(errorHelpSlug('unknown')).toBeUndefined();
		expect(errorHelpSlug('toString')).toBeUndefined();
		expect(errorHelpSlug(null)).toBeUndefined();
	});
});

describe('needsContact', () => {
	it('アカウントの衝突と凍結は、問い合わせ先を添える', () => {
		for (const code of [
			'google_account_conflict',
			'line_account_conflict',
			'apple_account_conflict',
			'external_account_conflict',
			'account_frozen'
		]) {
			expect(needsContact(code)).toBe(true);
		}
	});

	it('本人が直せる失敗・未知のコード・指定なしは添えない', () => {
		expect(needsContact('line_email_required')).toBe(false);
		expect(needsContact('google_failed')).toBe(false);
		expect(needsContact('unknown')).toBe(false);
		expect(needsContact(null)).toBe(false);
		expect(needsContact(undefined)).toBe(false);
	});
});
