// `{{shot:id}}` プレースホルダーに対応する撮影手順のレジストリ。
// 「どうやってその画面状態に到達するか」はコードでしか表現できないため、
// 仕様書の原稿の側にはロジックを書かず、ここに手順として持たせる。
//
// 各エントリの `run` は (page, ctx) => Promise<void> の形。ctx.baseURL は一時起動した
// bp-carnet backend のURL、ctx.outFile は撮影後のスクリーンショット保存先。
// `viewports` を省略すると既定で desktop/tablet/mobile の3サイズを撮り、
// generate-spec.ts 側で横並び比較表として展開する。レスポンシブで見た目が
// 変わらないと分かったショットは `viewports: ['desktop']` のように絞って1枚に縮小できる。
//
// generate-spec.ts 側でショットごとに独立した BrowserContext を渡すため、ここで
// ログインしたセッションが別のショットに漏れることはない。

import type { Locator, Page } from 'playwright';
import ja from '../messages/ja.json' with { type: 'json' };
import {
	mockAuthProviders,
	mockLinkedProviders,
	PRODUCTION_AUTH_PROVIDERS
} from '../e2e/auth-mocks.ts';
import {
	errorResponse,
	freeQuota,
	mockOcrStatus,
	mockOcrExtract,
	monitorResult,
	noValuesResult,
	notebookResult,
	samplePhoto,
	type NotebookRow
} from '../e2e/ocr-mocks.ts';
import { openLoginPage, waitForChart, waitForFonts } from '../e2e/page-waits.ts';

export interface ShotContext {
	baseURL: string;
	outFile: string;
}

export type ShotFn = (page: Page, ctx: ShotContext) => Promise<void>;

export type ViewportName = 'desktop' | 'tablet' | 'mobile';

export const VIEWPORTS: Record<ViewportName, { width: number; height: number }> = {
	desktop: { width: 1280, height: 800 },
	tablet: { width: 768, height: 1024 },
	mobile: { width: 390, height: 844 }
};

export const DEFAULT_VIEWPORTS: ViewportName[] = ['desktop', 'tablet', 'mobile'];

export interface ShotEntry {
	run: ShotFn;
	/** 省略時は DEFAULT_VIEWPORTS (レスポンシブ比較表になる)。 */
	viewports?: ViewportName[];
}

/** フォントを待ってから撮る。CSSアニメーション (alert-dialog の開く動き等) は、終わりを待つ
 * 代わりに無効にして、確定した見た目を撮る。 */
async function capture(
	page: Page,
	ctx: ShotContext,
	options: { fullPage?: boolean } = {}
): Promise<void> {
	await waitForFonts(page);
	await page.screenshot({ path: ctx.outFile, ...options, animations: 'disabled' });
}

// ログインボタンを role locator で掴むための表示文言。ハードコードすると文言変更時に
// 追随漏れが起きるため、messages/ja.json から直接読む。
const LOGIN_SUBMIT_LABEL = ja.login_submit_button;

/** OCR上限の変更ボタンの aria-label から、対象ユーザー名の部分を除いたもの。 */
const OCR_LIMIT_EDIT_LABEL_SUFFIX = ja.admin_users_ocr_limit_edit_label.replace('{name}', '');

/** 削除ボタンの aria-label から、対象ユーザー名の部分を除いたもの。 */
const DELETE_LABEL_SUFFIX = ja.admin_users_delete_label.replace('{name}', '');
const RECORD_EDIT_LABEL_SUFFIX = ja.record_card_edit_label.replace('{measuredAt}', '');

/** 写真で記録のページの残りの枠の「残り N%」。本番は無料の枠があるので出る。値は見本。 */
const SAMPLE_QUOTA_REMAINING_PERCENT = 60;

async function login(page: Page, baseURL: string, username: string, password: string) {
	// `just spec` の backend は GEMINI_API_KEY を空にして起動する (generate-spec.ts) ため、そのままでは
	// 本番にある「写真で記録...」が出ない。読み取りを有効と答えるモックで本番に合わせる。
	await mockOcrStatus(page, {
		quotas: [freeQuota(SAMPLE_QUOTA_REMAINING_PERCENT)],
		topupAvailable: true
	});
	// グラフは「視差効果を減らす」設定が無効だと描画をアニメーションするため、途中の状態を
	// 撮らないよう有効にしておく (CSS の animations: 'disabled' は JS の補間には効かない)。
	await page.emulateMedia({ reducedMotion: 'reduce' });
	await page.goto(`${baseURL}/login`);
	await page.fill('#username', username);
	await page.fill('#password', password);
	await page.getByRole('button', { name: LOGIN_SUBMIT_LABEL }).click();
	await page.waitForURL(`${baseURL}/`, { timeout: 5000 });
}

/** 無料の読み取りを使い切り、買い足しが出る (本番の構成の) 写真で記録のページを開く。 */
async function openQuotaExhaustedPhotoPage(page: Page, baseURL: string) {
	await login(page, baseURL, 'admin', 'password');
	await mockOcrStatus(page, { quotas: [freeQuota(0)], quotaLow: true, topupAvailable: true });
	await page.goto(`${baseURL}/record/photo`);
	await page
		.getByText(ja.photo_page_quota_exhausted_title, { exact: true })
		.waitFor({ timeout: 5000 });
}

/** メールのリンクが使えるかの問い合わせ (`POST /auth/{purpose}/check`) に「使える」(204) と答える。 */
async function mockEmailLinkUsable(page: Page, purpose: 'signup' | 'password-reset') {
	await page.route(`**/api/v1/auth/${purpose}/check`, (route) => route.fulfill({ status: 204 }));
}

/** マウスを画面の隅へ退ける。下端のバーのボタンを押した後、同じ位置に出るボタンがホバーの色で撮れないようにする。 */
async function moveMouseAway(page: Page) {
	await page.mouse.move(0, 0);
}

/** seed.sql の記録は朝夜どちらも値を持つので、トップページ・閲覧ページは既定の2本
 * (朝・夜それぞれの上の血圧) の描画を待てば足りる。レポート画面は4系列を待つ。 */
const HOME_CHART_SERIES = 2;

// `just spec` のbackendはLINE・Google・Appleを使わない開発・テストの既定で
// 起動する (generate-spec.ts、e2eのrun-e2e.tsと同じ理由)。そのままではログイン画面が
// 本番と食い違うため、e2eの mockAuthProviders (auth-mocks.ts) と同じ仕組みで、
// 本番の構成 (PRODUCTION_AUTH_PROVIDERS) に差し替える。

/** ホーム画面の「写真で記録...」で `photo` の見本の写真を選び、読み取りが `response` を返す (モック) 写真で記録のページへ移る。
 * `read` が偽なら、選んだ写真を確かめる段階で止め、「読み取る」を押さない。 */
async function openPhotoPage(
	page: Page,
	baseURL: string,
	response: Parameters<typeof mockOcrExtract>[1],
	photo: Parameters<typeof samplePhoto>[1],
	{ read = true }: { read?: boolean } = {}
) {
	await login(page, baseURL, 'admin', 'password');
	await mockOcrExtract(page, response);
	const file = await samplePhoto(page, photo);
	const chooser = page.waitForEvent('filechooser');
	await page.getByRole('button', { name: ja.home_photo_ocr_button, exact: true }).click();
	await (await chooser).setFiles(file);
	await moveMouseAway(page);
	await page.waitForURL('**/record/photo');
	const readButton = page.getByRole('button', { name: ja.photo_page_read_button, exact: true });
	await readButton.waitFor({ timeout: 5000 });
	if (read) await readButton.click();
}

/** `scope` の中の写真の枠の画像の読み込み (デコード) を待つ。枠が空のまま撮らないため。 */
async function waitForPhotoPreview(scope: Page | Locator) {
	const img = scope.getByRole('img', { name: ja.photo_page_preview_alt }).first();
	await img.waitFor({ timeout: 5000 });
	await img.evaluate((el: HTMLImageElement) => el.decode());
}

/** 日本時間の今日から `offset` 日前の日付 (`YYYY-MM-DD`)。撮影のブラウザ (generate-spec.ts) と同じく、流すマシンのタイムゾーンによらない。 */
function daysAgo(offset: number): string {
	const today = new Intl.DateTimeFormat('sv-SE', { timeZone: 'Asia/Tokyo' }).format(new Date());
	const date = new Date(`${today}T00:00:00Z`);
	date.setUTCDate(date.getUTCDate() - offset);
	return date.toISOString().slice(0, 10);
}

/** 手帳の見本。seed の記録 (今週) と重ならない過去の日付にする。2日目は時刻を書いていない
 * (一覧では「朝」「夜」と出る)。最後の行は読み取りの自信が低い (「要確認」)。 */
function notebookSample() {
	const day1 = daysAgo(41);
	const day2 = daysAgo(40);
	const rows: NotebookRow[] = [
		{ date: day1, time: '07:10', systolic: 132, diastolic: 86, pulse: 72 },
		{ date: day1, time: '21:30', systolic: 128, diastolic: 84, pulse: 70, memo: '散歩のあと' },
		{ date: day2, systolic: 126, diastolic: 82, pulse: 68 },
		{ date: day2, systolic: 145, diastolic: 90, pulse: 75, confidence: 0.6 }
	];
	return { rows, result: notebookResult(rows) };
}

/** CSV取り込みの見本。既存の記録と重ならない日付 (seed の記録は常に「今週」) にし、1行だけ数値でない収縮期を混ぜてエラー行にする。 */
const SAMPLE_IMPORT_CSV = [
	'measuredAt,systolic,diastolic,pulse,memo',
	'2020-06-10T07:00,120,80,70,起床後',
	'2020-06-10T21:00,abc,88,,測り直す',
	'2020-06-11T07:30,128,84,65,散歩後'
].join('\r\n');

/** 設定画面の「CSVファイルを読み込む...」で見本のCSVを選び、取り込みのページへ移る。 */
async function openCsvImport(page: Page, baseURL: string) {
	await login(page, baseURL, 'admin', 'password');
	await page.goto(`${baseURL}/settings`);
	const chooser = page.waitForEvent('filechooser');
	await page.getByRole('button', { name: ja.settings_import_button, exact: true }).click();
	await (
		await chooser
	).setFiles({ name: 'records.csv', mimeType: 'text/csv', buffer: Buffer.from(SAMPLE_IMPORT_CSV) });
	await page.waitForURL('**/settings/import');
}

export const shots: Record<string, ShotEntry> = {
	// 未ログイン状態でログインページへ直接アクセスした初期状態。
	login_initial: {
		async run(page, ctx) {
			await mockAuthProviders(page, PRODUCTION_AUTH_PROVIDERS);
			await openLoginPage(page, `${ctx.baseURL}/login`);
			await capture(page, ctx);
		}
	},

	// パスワードを誤ってログインし、エラーダイアログが表示された状態。
	login_error: {
		async run(page, ctx) {
			await mockAuthProviders(page, PRODUCTION_AUTH_PROVIDERS);
			await openLoginPage(page, `${ctx.baseURL}/login`);
			await page.fill('#username', 'admin');
			await page.fill('#password', 'wrong-password');
			await page.getByRole('button', { name: LOGIN_SUBMIT_LABEL }).click();
			await page.waitForSelector('[role="alertdialog"]', { timeout: 5000 });
			// alert-dialog の開くアニメーション (fade-in + zoom-in, 100ms) を待つ代わりに、
			// スクリーンショット側でアニメーションを無効化して即座に確定した見た目を撮る。
			await capture(page, ctx);
		}
	},

	// ログイン済みでトップページ(統計サマリー+グラフ+履歴一覧+下部アクションバー)を表示した状態。
	home_with_records: {
		async run(page, ctx) {
			await login(page, ctx.baseURL, 'admin', 'password');
			await waitForChart(page, HOME_CHART_SERIES);
			await capture(page, ctx, { fullPage: true });
		}
	},

	// 設定画面(期間閾値・タイムゾーン・CSV出力/インポート・アカウント・このアプリについて・危険な操作)。
	// 設定・タイムゾーン一覧の取得完了後に時刻の欄が描画されるため、それを待ってから撮る。
	settings_main: {
		async run(page, ctx) {
			await login(page, ctx.baseURL, 'admin', 'password');
			await page.goto(`${ctx.baseURL}/settings`);
			await page
				.getByRole('button', { name: ja.settings_morning_start_label })
				.waitFor({ timeout: 5000 });
			await capture(page, ctx, { fullPage: true });
		}
	},

	// 時刻の欄をタップして開く「時」「分」の選択。
	settings_time_picker: {
		viewports: ['mobile'],
		async run(page, ctx) {
			await login(page, ctx.baseURL, 'admin', 'password');
			await page.goto(`${ctx.baseURL}/settings`);
			await page
				.getByRole('button', { name: ja.settings_morning_start_label })
				.click({ timeout: 5000 });
			await page.getByRole('group', { name: ja.time_picker_hour_label }).waitFor();
			await capture(page, ctx);
		}
	},

	// パスワード変更画面。条件の説明はサーバーの設定 (`[password]`) から引くため、
	// 取得が終わって欄が描かれるのを待ってから撮る。
	settings_change_password: {
		viewports: ['mobile'],
		async run(page, ctx) {
			await login(page, ctx.baseURL, 'admin', 'password');
			await page.goto(`${ctx.baseURL}/settings/change-password`);
			await page
				.getByRole('textbox', { name: ja.settings_change_password_current_label })
				.waitFor({ timeout: 5000 });
			await capture(page, ctx);
		}
	},

	// ユーザー管理画面 (管理者専用)。一覧の取得が終わってカードが描かれるのを待ってから撮る。
	admin_users: {
		viewports: ['mobile'],
		async run(page, ctx) {
			await login(page, ctx.baseURL, 'admin', 'password');
			await page.goto(`${ctx.baseURL}/admin/users`);
			await page.getByRole('listitem').first().waitFor({ timeout: 5000 });
			await capture(page, ctx, { fullPage: true });
		}
	},

	// OCR上限の変更モーダル。金額の入力欄が出る「金額を指定」まで進めた状態を撮る。
	admin_users_ocr_limit: {
		viewports: ['mobile'],
		async run(page, ctx) {
			await login(page, ctx.baseURL, 'admin', 'password');
			await page.goto(`${ctx.baseURL}/admin/users`);
			const firstUser = page.getByRole('listitem').first();
			await firstUser.waitFor({ timeout: 5000 });
			// 入口のボタンは対象ユーザー名込みの aria-label を持つため、名前を除いた部分で掴む。
			await firstUser.getByRole('button', { name: OCR_LIMIT_EDIT_LABEL_SUFFIX }).click();
			await page.getByRole('dialog').waitFor({ timeout: 5000 });
			await page
				.getByLabel(ja.admin_users_ocr_budget_mode_label, { exact: true })
				.selectOption('custom');
			await capture(page, ctx);
		}
	},

	// ユーザー追加モーダル。パスワードの条件はサーバーから取るため、欄が出るまで待ってから撮る。
	admin_users_add: {
		viewports: ['mobile'],
		async run(page, ctx) {
			await login(page, ctx.baseURL, 'admin', 'password');
			await page.goto(`${ctx.baseURL}/admin/users`);
			await page.getByRole('listitem').first().waitFor({ timeout: 5000 });
			await page.getByRole('button', { name: ja.admin_users_add_button }).click();
			await page.locator('#add-user-username').waitFor({ timeout: 5000 });
			await capture(page, ctx);
		}
	},

	// パスワード再設定モーダル。ボタンは有効な (凍結されていない) ユーザーの行に出る。
	// 自分ではなく他の利用者を再設定するのが主な用途なので、seed の「母」の行から開く。
	admin_users_reset_password: {
		viewports: ['mobile'],
		async run(page, ctx) {
			await login(page, ctx.baseURL, 'admin', 'password');
			await page.goto(`${ctx.baseURL}/admin/users`);
			const activeUser = page.getByRole('listitem').filter({ hasText: '母' }).first();
			await activeUser.waitFor({ timeout: 5000 });
			await activeUser.getByRole('button', { name: ja.admin_users_reset_password_button }).click();
			// パスワードの条件はサーバーから取るため、欄が出るまで待ってから撮る。
			await page.locator('#reset-password-new').waitFor({ timeout: 5000 });
			await capture(page, ctx);
		}
	},

	// ユーザー削除の確認モーダル。削除ボタンは凍結中のユーザーにだけ出るため、その行から開く。
	admin_users_delete: {
		viewports: ['mobile'],
		async run(page, ctx) {
			await login(page, ctx.baseURL, 'admin', 'password');
			await page.goto(`${ctx.baseURL}/admin/users`);
			const frozenUser = page
				.getByRole('listitem')
				.filter({ hasText: ja.admin_users_status_frozen })
				.first();
			await frozenUser.waitFor({ timeout: 5000 });
			await frozenUser.getByRole('button', { name: DELETE_LABEL_SUFFIX }).click();
			await page.getByRole('dialog').waitFor({ timeout: 5000 });
			await capture(page, ctx);
		}
	},

	// 利用者の閲覧ページ。seed の記録を持つ admin 自身を開き、グラフの描画まで待ってから撮る
	// (他の seed ユーザーは記録を持たない)。閲覧ページは今週から開くので、seed の記録がある先週へ戻す。
	admin_user_detail: {
		viewports: ['mobile'],
		async run(page, ctx) {
			await login(page, ctx.baseURL, 'admin', 'password');
			await page.goto(`${ctx.baseURL}/admin/users`);
			const self = page.getByRole('listitem').filter({ hasText: 'admin' }).first();
			await self.waitFor({ timeout: 5000 });
			await self.getByRole('link').click();
			await page.getByRole('button', { name: ja.period_nav_prev_button, exact: true }).click();
			await waitForChart(page, HOME_CHART_SERIES);
			await capture(page, ctx, { fullPage: true });
		}
	},

	// 印刷用レポート画面。印刷メディアをエミュレートして、紙面を撮影する。
	print_report: {
		viewports: ['desktop'],
		async run(page, ctx) {
			await login(page, ctx.baseURL, 'admin', 'password');
			await page.goto(`${ctx.baseURL}/report`);
			await waitForChart(page, 4);
			await page.emulateMedia({ media: 'print' });
			await capture(page, ctx, { fullPage: true });
		}
	},

	// 印刷用レポート画面のスマホ幅(印刷前の画面表示)。見出しが2行になる (print media では
	// `print:flex-row` が優先され1行に戻るため、print_reportとは別に画面表示のまま撮る)。
	print_report_mobile: {
		viewports: ['mobile'],
		async run(page, ctx) {
			await login(page, ctx.baseURL, 'admin', 'password');
			await page.goto(`${ctx.baseURL}/report`);
			await waitForChart(page, 4);
			await capture(page, ctx);
		}
	},

	// アカウント作成の入力画面。
	signup_initial: {
		async run(page, ctx) {
			await mockAuthProviders(page, PRODUCTION_AUTH_PROVIDERS);
			await page.goto(`${ctx.baseURL}/signup`);
			await page.locator('#email').waitFor({ timeout: 5000 });
			await capture(page, ctx);
		}
	},

	// パスワード再設定の申し込み画面。
	reset_password_request: {
		async run(page, ctx) {
			await mockAuthProviders(page, PRODUCTION_AUTH_PROVIDERS);
			await page.goto(`${ctx.baseURL}/reset-password`);
			await page.locator('#email').waitFor({ timeout: 5000 });
			await capture(page, ctx);
		}
	},

	// パスワード再設定メールのリンク先。開いた時点でリンクが使えるかをサーバーに確かめるので、
	// 使えると答えるモックにして、ダミーのトークンで新パスワード入力フォームを撮る。
	reset_password_form: {
		async run(page, ctx) {
			await mockEmailLinkUsable(page, 'password-reset');
			await page.goto(`${ctx.baseURL}/reset-password?token=dummy-token`);
			await page.locator('#password-new').waitFor({ timeout: 5000 });
			await capture(page, ctx);
		}
	},

	// 確認メールのリンク先 (アカウント作成の仕上げ)。reset_password_form と同じ理由でダミーの
	// トークンで撮る。
	verify_email_form: {
		async run(page, ctx) {
			await mockEmailLinkUsable(page, 'signup');
			await page.goto(`${ctx.baseURL}/verify-email?token=dummy-token`);
			await page.locator('#password-new').waitFor({ timeout: 5000 });
			await capture(page, ctx);
		}
	},

	// 利用規約ページ。
	terms_page: {
		async run(page, ctx) {
			await mockAuthProviders(page, PRODUCTION_AUTH_PROVIDERS);
			await page.goto(`${ctx.baseURL}/terms`);
			await page.getByRole('article').waitFor({ timeout: 5000 });
			await capture(page, ctx);
		}
	},

	// プライバシーポリシーページ。
	privacy_page: {
		async run(page, ctx) {
			await mockAuthProviders(page, PRODUCTION_AUTH_PROVIDERS);
			await page.goto(`${ctx.baseURL}/privacy`);
			await page.getByRole('article').waitFor({ timeout: 5000 });
			await capture(page, ctx);
		}
	},

	// 使い方の目次。configに依存せず、未ログインでも開ける。
	help_index: {
		async run(page, ctx) {
			await page.goto(`${ctx.baseURL}/help`);
			await page.getByRole('heading', { level: 1 }).waitFor({ timeout: 5000 });
			await capture(page, ctx);
		}
	},

	// 使い方の項目ページ (例: ログイン)。手順の画像は frontend/static/help/ に同梱済みのものを使う。
	help_topic: {
		async run(page, ctx) {
			await page.goto(`${ctx.baseURL}/help/login`);
			await page.getByRole('heading', { level: 1 }).waitFor({ timeout: 5000 });
			await page.locator('img').first().waitFor({ timeout: 5000 });
			await capture(page, ctx);
		}
	},

	// 共通ヘッダーのアバターを押して開いたユーザーメニュー(設定・使い方・ログアウト)。
	header_user_menu: {
		viewports: ['mobile'],
		async run(page, ctx) {
			await login(page, ctx.baseURL, 'admin', 'password');
			await page.getByRole('button', { name: ja.layout_user_menu_label }).click();
			await page
				.getByRole('menuitem', { name: ja.common_logout_button })
				.waitFor({ timeout: 5000 });
			await capture(page, ctx);
		}
	},

	// ログアウトの確認ダイアログ。ヘッダーのユーザーメニューから開く。
	logout_confirm: {
		viewports: ['mobile'],
		async run(page, ctx) {
			await login(page, ctx.baseURL, 'admin', 'password');
			await page.getByRole('button', { name: ja.layout_user_menu_label }).click();
			await page.getByRole('menuitem', { name: ja.common_logout_button }).click();
			await page.getByRole('alertdialog').waitFor({ timeout: 5000 });
			await capture(page, ctx);
		}
	},

	// アカウント削除の確認モーダル(確認文字列の入力欄)。設定画面から開く。
	delete_account_confirm: {
		viewports: ['mobile'],
		async run(page, ctx) {
			await login(page, ctx.baseURL, 'admin', 'password');
			await page.goto(`${ctx.baseURL}/settings`);
			await page.getByRole('button', { name: ja.common_delete_account_button }).click();
			await page.locator('#delete-account-confirmation').waitFor({ timeout: 5000 });
			await capture(page, ctx);
		}
	},

	// アカウント削除を予約した後の結果表示。実際にadminアカウントを削除予約すると他のショットの
	// ログインが壊れるため、POST /api/v1/account/deletion をその場で成功応答に差し替えて撮る。
	delete_account_scheduled: {
		viewports: ['mobile'],
		async run(page, ctx) {
			await login(page, ctx.baseURL, 'admin', 'password');
			await page.route('**/api/v1/account/deletion', (route) => route.fulfill({ json: {} }));
			await page.goto(`${ctx.baseURL}/settings`);
			await page.getByRole('button', { name: ja.common_delete_account_button }).click();
			await page
				.locator('#delete-account-confirmation')
				.fill(ja.delete_account_dialog_confirm_word);
			await page.getByRole('button', { name: ja.delete_account_dialog_submit_button }).click();
			await moveMouseAway(page);
			await page.getByText(ja.delete_account_dialog_scheduled_title).waitFor({ timeout: 5000 });
			await capture(page, ctx);
		}
	},

	// パスワード変更が成功し、フォームが結果表示に差し替わった状態。adminの実際のパスワードを
	// 変えると他のショットのログインが壊れるため、PUT /api/v1/account/password を成功応答に差し替える。
	settings_change_password_done: {
		viewports: ['mobile'],
		async run(page, ctx) {
			await login(page, ctx.baseURL, 'admin', 'password');
			await page.route('**/api/v1/account/password', (route) => route.fulfill({ json: {} }));
			await page.goto(`${ctx.baseURL}/settings/change-password`);
			await page
				.getByRole('textbox', { name: ja.settings_change_password_current_label })
				.fill('password');
			await page.locator('#password-new').fill('a-valid-password-12');
			await page.locator('#password-confirm').fill('a-valid-password-12');
			await page.getByRole('button', { name: ja.settings_change_password_submit_button }).click();
			await moveMouseAway(page);
			await page.getByText(ja.settings_change_password_done_title).waitFor({ timeout: 5000 });
			await capture(page, ctx);
		}
	},

	// 設定画面の「ログイン方法」節。管理者seedユーザーは連携が無く節ごと出ないため、
	// LINE・Googleの両方と連携済みの状態をmockLinkedProviders(e2eと同じ手法)で作る。
	settings_linked_providers: {
		viewports: ['mobile'],
		async run(page, ctx) {
			await login(page, ctx.baseURL, 'admin', 'password');
			await mockLinkedProviders(page, {
				passwordUsable: true,
				linkedProviders: ['line', 'google']
			});
			await page.goto(`${ctx.baseURL}/settings`);
			const heading = page.getByText(ja.settings_login_methods_section_title);
			await heading.waitFor({ timeout: 5000 });
			await heading.evaluate((el) => el.scrollIntoView({ block: 'center' }));
			await capture(page, ctx);
		}
	},

	// 「このアプリについて」の欄。
	settings_about: {
		viewports: ['mobile'],
		async run(page, ctx) {
			await login(page, ctx.baseURL, 'admin', 'password');
			await page.goto(`${ctx.baseURL}/settings`);
			const contact = page.getByRole('link', { name: ja.settings_contact_link });
			await contact.waitFor({ timeout: 5000 });
			await contact.evaluate((el) => el.scrollIntoView({ block: 'center' }));
			await capture(page, ctx);
		}
	},

	// 削除を予約した後の「危険な操作」。予約の状態は `/auth/me` の応答を差し替えて作る (撮影用のアカウントを消さないため)。
	settings_deletion_scheduled: {
		viewports: ['mobile'],
		async run(page, ctx) {
			await login(page, ctx.baseURL, 'admin', 'password');
			await page.route('**/api/v1/auth/me', async (route) => {
				const response = await route.fetch();
				const me = await response.json();
				await route.fulfill({
					response,
					json: {
						...me,
						deletionScheduledAt: '2026-10-25T00:00:00.000Z',
						deletionOrigin: 'user'
					}
				});
			});
			await page.goto(`${ctx.baseURL}/settings`);
			const hint = page.getByText(ja.settings_deletion_cancel_hint, { exact: true });
			await hint.waitFor({ timeout: 5000 });
			await hint.evaluate((el) => el.scrollIntoView({ block: 'center' }));
			await capture(page, ctx);
		}
	},

	// 連携解除の確認モーダル。
	unlink_identity_confirm: {
		viewports: ['mobile'],
		async run(page, ctx) {
			await login(page, ctx.baseURL, 'admin', 'password');
			await mockLinkedProviders(page, {
				passwordUsable: true,
				linkedProviders: ['line', 'google']
			});
			await page.goto(`${ctx.baseURL}/settings`);
			await page.getByText(ja.settings_login_methods_section_title).waitFor({ timeout: 5000 });
			await page.getByRole('button', { name: ja.settings_unlink_button }).first().click();
			await page.getByRole('dialog').waitFor({ timeout: 5000 });
			await capture(page, ctx);
		}
	},

	// トップページの「期間を指定」を開いた展開状態(開始・終了の日付欄)。
	home_period_filter_expanded: {
		viewports: ['mobile'],
		async run(page, ctx) {
			await login(page, ctx.baseURL, 'admin', 'password');
			await waitForChart(page, HOME_CHART_SERIES);
			await page.getByText(ja.period_nav_filter_toggle_label, { exact: true }).click();
			await page.locator('#filter-from').waitFor({ timeout: 5000 });
			await capture(page, ctx);
		}
	},

	// トップページで、表示中の期間に記録が0件のとき(統計サマリー・グラフ・一覧が全て非表示)。
	// 実在の記録が無い期間まで絞り込むことで、seedの記録を消さずに再現する。
	home_no_records: {
		viewports: ['mobile'],
		async run(page, ctx) {
			await login(page, ctx.baseURL, 'admin', 'password');
			await page.getByText(ja.period_nav_filter_toggle_label, { exact: true }).click();
			await page.locator('#filter-from').fill('2000-01-01');
			await page.locator('#filter-to').fill('2000-01-07');
			await page.getByRole('button', { name: ja.period_nav_filter_apply_button }).click();
			await page.getByText(ja.record_empty).waitFor({ timeout: 5000 });
			await capture(page, ctx);
		}
	},

	// LINE・Instagram等の内蔵ブラウザで開いたときの警告バナー。navigator.userAgentを
	// ページのスクリプトが読む前に `addInitScript` で差し替える。
	login_in_app_browser: {
		async run(page, ctx) {
			await mockAuthProviders(page, PRODUCTION_AUTH_PROVIDERS);
			await page.addInitScript(() => {
				Object.defineProperty(window.navigator, 'userAgent', {
					value:
						'Mozilla/5.0 (Linux; Android 10; SM-G973F) AppleWebKit/537.36 (KHTML, like Gecko) Version/4.0 Chrome/122.0.0.0 Mobile Safari/537.36 Line/13.7.0',
					configurable: true
				});
			});
			await openLoginPage(page, `${ctx.baseURL}/login`);
			await page.getByText(ja.login_in_app_browser_copy_button).waitFor({ timeout: 5000 });
			await capture(page, ctx);
		}
	},

	// iOSホーム画面から全画面(Webアプリとして開く)で起動したときの警告バナー。
	// navigator.standaloneはiOS Safari固有のプロパティで、モックのため直接定義する。
	login_ios_web_app: {
		viewports: ['mobile'],
		async run(page, ctx) {
			await mockAuthProviders(page, PRODUCTION_AUTH_PROVIDERS);
			await page.addInitScript(() => {
				Object.defineProperty(window.navigator, 'standalone', {
					value: true,
					configurable: true
				});
			});
			await openLoginPage(page, `${ctx.baseURL}/login`);
			// 勧める方法 ({services}) は有効な方法で変わるので、その前までで探す。
			await page
				.getByText(ja.login_line_ios_web_app_notice.split('{services}')[0])
				.waitFor({ timeout: 5000 });
			await capture(page, ctx);
		}
	},

	// 記録の登録フォーム(新規)。下部アクションバーの「手動で入力...」から開く。
	record_form_add: {
		viewports: ['mobile'],
		async run(page, ctx) {
			await login(page, ctx.baseURL, 'admin', 'password');
			await waitForChart(page, HOME_CHART_SERIES);
			await page.getByRole('button', { name: ja.home_add_record_button }).click();
			await moveMouseAway(page);
			await page.locator('#record-systolic').waitFor({ timeout: 5000 });
			await capture(page, ctx);
		}
	},

	// 記録の編集フォーム。履歴一覧の鉛筆のボタンから開く(見出しが「記録を直す」に変わる)。
	record_form_edit: {
		viewports: ['mobile'],
		async run(page, ctx) {
			await login(page, ctx.baseURL, 'admin', 'password');
			await waitForChart(page, HOME_CHART_SERIES);
			// aria-label は「{measuredAt} の記録を直す」で日時ごとに変わるため、共通の接尾辞で掴む。
			await page.getByRole('button', { name: RECORD_EDIT_LABEL_SUFFIX }).first().click();
			await page.locator('#record-systolic').waitFor({ timeout: 5000 });
			await capture(page, ctx);
		}
	},

	// CSV取り込みの一覧。見本のCSV (SAMPLE_IMPORT_CSV) にはエラー行が1つある。
	csv_import_rows: {
		viewports: ['mobile'],
		async run(page, ctx) {
			await openCsvImport(page, ctx.baseURL);
			await page.getByRole('heading', { name: ja.import_rows_title, exact: true }).waitFor();
			await page
				.getByText(ja.import_rows_count.replace('{total}', '3').replace('{kept}', '2'), {
					exact: true
				})
				.waitFor({ timeout: 5000 });
			await capture(page, ctx);
		}
	},

	// 取り込む前の行を直すシート。エラー行(数値でない収縮期)を開いた状態。
	csv_import_row_sheet: {
		viewports: ['mobile'],
		async run(page, ctx) {
			await openCsvImport(page, ctx.baseURL);
			await page.getByRole('listitem').filter({ hasText: '測り直す' }).getByRole('button').click();
			// このページの行を直すシートは idPrefix="import-row" (settings/import/+page.svelte)。
			await page.locator('#import-row-systolic').waitFor({ timeout: 5000 });
			await capture(page, ctx);
		}
	},

	// 取り込み内容の確認画面。エラー行を直してから「最終確認」で進む。
	csv_import_preview: {
		viewports: ['mobile'],
		async run(page, ctx) {
			await openCsvImport(page, ctx.baseURL);
			const errorRow = page.getByRole('listitem').filter({ hasText: '測り直す' });
			await errorRow.getByRole('button').click();
			await page.locator('#import-row-systolic').fill('118');
			await page.getByRole('button', { name: ja.record_form_dialog_update_button }).click();
			await page.getByRole('button', { name: ja.common_review_button, exact: true }).click();
			await moveMouseAway(page);
			await page.getByRole('heading', { name: ja.import_preview_title }).waitFor({ timeout: 5000 });
			// 差分の取得が終わってdiff一覧が描かれるまで待つ(見出しだけでは「読み込み中...」のまま撮ってしまう)。
			await page.getByRole('listitem').first().waitFor({ timeout: 5000 });
			await capture(page, ctx);
		}
	},

	// 写真で記録のページを写真なしで開いた状態 (再読み込み・URL を直接開いたとき)。上に無料の読み取りの残りを出す。
	photo_pick: {
		viewports: ['mobile'],
		async run(page, ctx) {
			await login(page, ctx.baseURL, 'admin', 'password');
			await page.goto(`${ctx.baseURL}/record/photo`);
			await page
				.getByText(
					ja.ocr_quota_remaining.replace('{percent}', String(SAMPLE_QUOTA_REMAINING_PERCENT)),
					{ exact: true }
				)
				.waitFor({ timeout: 5000 });
			await capture(page, ctx);
		}
	},

	// 無料の読み取りを使い切った状態で開いたところ。残りの枠が使い切りの見出しと色に替わる。
	// 本番は Stripe 決済が有効なので、使い切ると「読み取りを買い足す」が出る。
	photo_quota_exhausted: {
		viewports: ['mobile'],
		async run(page, ctx) {
			await openQuotaExhaustedPhotoPage(page, ctx.baseURL);
			await capture(page, ctx);
		}
	},

	// 「読み取りを買い足す」の確認ダイアログ。「同意して購入へ進む」で Stripe Checkout へ移る。
	topup_confirm: {
		viewports: ['mobile'],
		async run(page, ctx) {
			await openQuotaExhaustedPhotoPage(page, ctx.baseURL);
			await page.getByRole('button', { name: ja.photo_page_topup_button, exact: true }).click();
			await page.getByRole('dialog').waitFor({ timeout: 5000 });
			await capture(page, ctx);
		}
	},

	// Checkout から戻った購入結果の画面。枠が付いた (Webhook を受け取った) 後の表示。
	topup_result: {
		viewports: ['mobile'],
		async run(page, ctx) {
			await login(page, ctx.baseURL, 'admin', 'password');
			await page.route(
				(url) => url.pathname.endsWith('/api/v1/payments/ocr-topup/result'),
				(route) => route.fulfill({ json: { status: 'succeeded' } })
			);
			await page.goto(`${ctx.baseURL}/payments/ocr-topup/result?session_id=cs_sample`);
			await page
				.getByText(ja.topup_result_succeeded_title, { exact: true })
				.waitFor({ timeout: 5000 });
			await capture(page, ctx);
		}
	},

	// 特定商取引法に基づく表記。ログインしてから開くページ。
	tokushoho_page: {
		async run(page, ctx) {
			await mockAuthProviders(page, PRODUCTION_AUTH_PROVIDERS);
			await login(page, ctx.baseURL, 'admin', 'password');
			await page.goto(`${ctx.baseURL}/tokushoho`);
			await page.getByRole('article').waitFor({ timeout: 5000 });
			await capture(page, ctx);
		}
	},

	// 選んだ写真を読み取る前に確かめる段階。「読み取る」を押すまで読み取らない。
	photo_confirm: {
		viewports: ['mobile'],
		async run(page, ctx) {
			const values = { systolic: 124, diastolic: 82, pulse: 60 };
			await openPhotoPage(
				page,
				ctx.baseURL,
				{ json: monitorResult(values) },
				{ kind: 'monitor', values },
				{ read: false }
			);
			await waitForPhotoPreview(page);
			await capture(page, ctx);
		}
	},

	// 血圧計の液晶の読み取り結果。写真の枠の下に信頼度と登録フォームが並ぶ。
	photo_monitor_result: {
		viewports: ['mobile'],
		async run(page, ctx) {
			const values = { systolic: 124, diastolic: 82, pulse: 60 };
			await openPhotoPage(
				page,
				ctx.baseURL,
				{ json: monitorResult(values) },
				{
					kind: 'monitor',
					values
				}
			);
			await page.locator('#ocr-systolic').waitFor({ timeout: 5000 });
			await waitForPhotoPreview(page);
			await capture(page, ctx);
		}
	},

	// 写真の枠の隅のボタンで開く全画面表示。
	photo_lightbox: {
		viewports: ['mobile'],
		async run(page, ctx) {
			const values = { systolic: 124, diastolic: 82, pulse: 60 };
			await openPhotoPage(
				page,
				ctx.baseURL,
				{ json: monitorResult(values) },
				{
					kind: 'monitor',
					values
				}
			);
			await waitForPhotoPreview(page);
			await page.getByRole('button', { name: ja.photo_page_preview_open_label }).click();
			await page.getByText(ja.photo_lightbox_hint).waitFor({ timeout: 5000 });
			await capture(page, ctx);
		}
	},

	// 手帳の複数行の読み取り結果の一覧。時刻の無い行は「朝」「夜」、自信の低い行は「要確認」で出る。
	photo_memo_rows: {
		viewports: ['mobile'],
		async run(page, ctx) {
			const { rows, result } = notebookSample();
			await openPhotoPage(page, ctx.baseURL, { json: result }, { kind: 'notebook', rows });
			await page
				.getByRole('listitem')
				.nth(rows.length - 1)
				.waitFor({ timeout: 5000 });
			// 「最終確認」は、置き換える記録からメモを引き継ぐ取得が終わるまで押せない。押せない色で撮らないよう待つ。
			await page
				.getByRole('button', { name: ja.common_review_button, exact: true })
				.and(page.locator(':enabled'))
				.waitFor({ timeout: 5000 });
			await waitForPhotoPreview(page);
			await capture(page, ctx);
		}
	},

	// 手帳の行を直すシート。見出しの下に写真の枠が付く (取り込みのシートとの違い)。自信の低い行を開いた状態。
	photo_memo_row_sheet: {
		viewports: ['mobile'],
		async run(page, ctx) {
			const { rows, result } = notebookSample();
			await openPhotoPage(page, ctx.baseURL, { json: result }, { kind: 'notebook', rows });
			await page.getByRole('listitem').filter({ hasText: '145' }).getByRole('button').click();
			// このページの行を直すシートは idPrefix="memo-row" (record/photo/+page.svelte)。
			await page.locator('#memo-row-systolic').waitFor({ timeout: 5000 });
			await waitForPhotoPreview(page.getByRole('dialog'));
			await capture(page, ctx);
		}
	},

	// 血圧値の写っていない写真を読み取った結果。
	photo_no_values: {
		viewports: ['mobile'],
		async run(page, ctx) {
			await openPhotoPage(page, ctx.baseURL, { json: noValuesResult() }, { kind: 'monitor' });
			await page.getByText(ja.photo_page_no_values, { exact: true }).waitFor({ timeout: 5000 });
			await waitForPhotoPreview(page);
			await capture(page, ctx);
		}
	},

	// 読み取りの失敗。
	photo_error: {
		viewports: ['mobile'],
		async run(page, ctx) {
			await openPhotoPage(page, ctx.baseURL, errorResponse(502, 'ocr_upstream_error'), {
				kind: 'monitor',
				values: { systolic: 124, diastolic: 82, pulse: 60 }
			});
			await page.getByText(ja.error_ocr_upstream_error).waitFor({ timeout: 5000 });
			await capture(page, ctx);
		}
	}
};
