import * as m from '$lib/paraglide/messages.js';

/** バックエンドの `src/validation.rs` と同じ上限 (コードポイント数)。 */
const USERNAME_MAX_CHARS = 254;

/** 見た目では気づけない文字 (空白・制御文字 Cc・書式文字 Cf)。バックエンドの
 * `validation::is_invisible` と同じ集合。ゼロ幅スペース (U+200B) のように、既存のIDと
 * 見分けがつかない別IDを作れてしまうため受け付けない。 */
const INVALID_CHARACTER = /[\s\p{Cc}\p{Cf}]/u;

/** ユーザーIDの入力エラー。問題が無ければ `undefined`。
 *
 * 判定はバックエンドの `validation::validate_username` と同じにする。ずれていると、
 * 欄の下にエラーを出せないまま汎用のエラーダイアログになり、直し方が分からなくなる。 */
export function usernameError(username: string): string | undefined {
	if (username.length === 0 || INVALID_CHARACTER.test(username)) {
		return m.common_username_error();
	}
	// サロゲートペアを1文字として数えるため、`length` ではなくコードポイント数で見る。
	if ([...username].length > USERNAME_MAX_CHARS) {
		return m.common_username_too_long_error({ max: USERNAME_MAX_CHARS });
	}
	return undefined;
}
