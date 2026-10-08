import { defineConfig, devices } from '@playwright/test';
import { ADMIN_STORAGE_STATE } from './e2e/auth';
import { KEEP_DATA } from './e2e/kept-data';

// e2eはSvelteKitのSPAビルド単体ではなく、backend(/api/v1)込みのbp-carnetに対して行う。
// backendの起動方法は複数ある(release exe・`just dev-backend`・`just e2e-local`の一時起動・
// 検証用の app など)ためここでは決め打てず、webServerでの自動起動はしない。呼び出し側が起動済みのURLを渡す。
const baseURL = process.env.E2E_BASE_URL;
if (!baseURL) {
	throw new Error(
		'E2E_BASE_URL が未設定です。backendを起動してからそのURLを設定して実行する: ' +
			'E2E_BASE_URL=http://127.0.0.1:3010 E2E_ADMIN_PASSWORD=... pnpm run test:e2e'
	);
}

/** 証明書の検証を省くか。自己署名の証明書の環境に当てるときに `E2E_IGNORE_HTTPS_ERRORS=1` で明示する。 */
function ignoreHTTPSErrors(): boolean {
	return process.env.E2E_IGNORE_HTTPS_ERRORS === '1';
}

export default defineConfig({
	use: {
		baseURL,
		ignoreHTTPSErrors: ignoreHTTPSErrors(),
		// PWA の service worker (vite.config.ts、skipWaiting + clientsClaim) は、新しいコンテキストで初めて
		// 入った直後にページを読み込み直させ、送信中の操作を途中で打ち切ることがある。PWA 自体を
		// 確かめるテストでだけ `test.use({ serviceWorkers: 'allow' })` にする。
		serviceWorkers: 'block',
		// 日時の入力欄などブラウザが書式を決める部分を、利用者の環境 (日本語) と同じにする。
		locale: 'ja-JP',
		// ブラウザのタイムゾーン。自動設定のユーザーはこれに揃うため、実行するマシンによらず固定する。
		timezoneId: 'Asia/Tokyo',
		// 使い方の手順の画像 (→ e2e/help-shots.ts) はライトテーマで撮る。グラフは
		// 「視差効果を減らす」が無効だと描画をアニメーションするため、途中の状態を撮らないよう有効にする。
		// 日時の入力欄の書式は `locale` ではなくブラウザ自体の表示言語で決まるため、起動時に日本語にする。
		...(process.env.E2E_HELP_SHOTS === '1'
			? {
					colorScheme: 'light' as const,
					reducedMotion: 'reduce' as const,
					launchOptions: { args: ['--lang=ja-JP'] }
				}
			: {})
	},
	// 写真の読み取り (Gemini) のシナリオは利用回数を消費し、API キーの無い環境では流せないため、
	// `@ocr` タグを付けて普段の実行から外す。`E2E_OCR=1` のときだけ流す。
	grepInvert: process.env.E2E_OCR === '1' ? undefined : /@ocr/,
	// テストが作ったものを残すとき (`E2E_KEEP_DATA=1`、→ e2e/kept-data.ts) だけ、始めに前回の控えを消し、終わりに一覧を出す。
	...(KEEP_DATA
		? { globalSetup: './e2e/kept-data-reset.ts', globalTeardown: './e2e/kept-data-report.ts' }
		: {}),
	// 管理者ログインをテストごとにUI操作からやり直さない
	// (公式ベストプラクティス、認証状態の使い回し: https://playwright.dev/docs/auth)。
	// setupプロジェクトが一度だけログインしてstorageStateを保存し、e2eプロジェクトはそれを使い回す
	// (未ログイン状態が要るテストはファイル側でstorageStateを空に上書きする、→ auth.e2e.ts)。
	projects: [
		{ name: 'setup', testMatch: '**/*.setup.ts' },
		{
			name: 'e2e',
			testMatch: '**/*.e2e.{ts,js}',
			// ADR: 主な利用者はスマートフォンで使うため、スマホの幅を既定にする。Pixel なのは、
			// iPhone の端末定義が WebKit を要し、ブラウザを Chromium だけで済ませられなくなるため。
			use: { ...devices['Pixel 7'], storageState: ADMIN_STORAGE_STATE },
			dependencies: ['setup']
		}
	]
});
