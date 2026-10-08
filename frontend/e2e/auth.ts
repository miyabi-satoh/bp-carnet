import path from 'node:path';

// 既定の"admin"以外を使う運用もあるため env で差し替え可能にする。パスワードは
// インスタンスごとに異なり共有もできないため既定値を持たず、未設定なら即座に失敗させる。
export const ADMIN_USERNAME = process.env.E2E_ADMIN_USER ?? 'admin';
export const ADMIN_PASSWORD = (() => {
	const password = process.env.E2E_ADMIN_PASSWORD;
	if (!password) {
		throw new Error(
			'E2E_ADMIN_PASSWORD が未設定です。ログイン検証用の管理者パスワードを設定する。'
		);
	}
	return password;
})();

/**
 * auth.setup.ts が保存し、認証済みが前提のテストが読み込むstorageStateの保存先。
 * 使い捨ての backend に当てる `just e2e-local` は、手動の `just e2e` と取り合わないよう env で別の場所に向ける。
 */
export const ADMIN_STORAGE_STATE =
	process.env.E2E_ADMIN_STORAGE_STATE ?? path.join(import.meta.dirname, '.auth', 'admin.json');
