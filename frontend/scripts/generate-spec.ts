// `just spec` から呼ばれるアプリ仕様書ジェネレータ。
//
// 1. 一時 BP_CARNET_HOME を作り、専用ポートで `target/debug/bp-carnet` を起動する
//    (稼働中の dev-backend と衝突しないように、かつ既存の開発用DBを汚さないように)。
// 2. seed.sql を node:sqlite で直接流し込む(Rust側の変更なしでシードできる)。
// 3. Playwright でスクリーンショットを撮る。各ショットは既定で desktop/tablet/mobile の
//    3サイズを撮る(shots.ts の viewports で絞り込み可能)。3サイズ撮ったものは
//    `{{shot:id}}` の置換先が横並び比較表になる。
// 4. 仕様書の原稿 (SPEC_SRC_DIR) の `{{shot:id}}` プレースホルダーを実際の値に置換し、
//    docs/generated/spec/ (+ assets/*.png) として書き出す。生成物は .gitignore 対象。
//
// 仕様書の文言に Paraglide のメッセージキーを差し込む仕組みは持たない。表示言語が日本語のみで、
// 仕様書側に文言を直接書けるため。
//
// frontend/build は呼び出し元(justfile)が事前にビルド済みである前提
// (rust-embedはdebug buildだと実行時にディスクから読むため、ビルドし忘れると
// 古いUIのままスクショが撮れてしまう)。

import { chromium, type Browser } from 'playwright';
import {
	rmSync,
	mkdirSync,
	renameSync,
	existsSync,
	readFileSync,
	writeFileSync,
	readdirSync
} from 'node:fs';
import path from 'node:path';
import { DatabaseSync } from 'node:sqlite';
import { startBackend } from './backend-process.ts';
import { REPO_ROOT } from './repo-paths.ts';
import { shots, VIEWPORTS, DEFAULT_VIEWPORTS, type ViewportName } from './shots.ts';

const VIEWPORT_LABELS: Record<ViewportName, string> = {
	desktop: 'デスクトップ',
	tablet: 'タブレット',
	mobile: 'モバイル'
};

const SEED_SQL_PATH = path.join(import.meta.dirname, 'seed.sql');
// 仕様書の原稿 (環境変数 SPEC_SRC_DIR のフォルダ。このリポジトリの外にある) の *.md を、それぞれ docs/generated/spec/<同名> に生成する。
// mo はフォルダ内の複数Markdownをサイドバー切り替えで表示するビューアのため、
// 単一の巨大ファイルではなくページ単位で分割している。
// 空文字にしておくのは、後の関数の中でも string として扱えるようにするため (undefined の絞り込みは関数の中まで続かない)。
const SPEC_SRC_DIR = process.env.SPEC_SRC_DIR ?? '';
if (!SPEC_SRC_DIR) {
	throw new Error('SPEC_SRC_DIR (仕様書の原稿のフォルダ) を指定してください (`just spec <dir>`)');
}
const OUT_DIR = path.join(REPO_ROOT, 'docs/generated/spec');
// 生成中の一時出力・退避先。OUT_DIR と同じ親(docs/generated/、まるごと.gitignore対象)配下に
// 置くことで、置き換え時の rename が同一ファイルシステム内で完結するようにする
// (OSの一時ディレクトリ(tmpdir())に置くと、環境によっては別ファイルシステムになり
// renameSync が EXDEV で失敗しうるため)。PIDをパスに含めるのは、just spec を誤って
// 同時に2つ起動した場合に互いの一時出力・退避物を削除/移動し合わないようにするため。
const TMP_OUT_DIR = path.join(REPO_ROOT, `docs/generated/.spec-out.tmp.${process.pid}`);
const BACKUP_OUT_DIR = path.join(REPO_ROOT, `docs/generated/.spec-out.bak.${process.pid}`);

// 撮るマシンの .env の OCR_* に左右されないよう、環境変数を空にしてこの値を使わせる (main の startBackend)。
// 無料枠は、管理画面の OCR 上限モーダルの「既定値」に出る。写真の画面は有限の無料枠の応答で撮る (shots.ts) ので、
// 有限の仮の値にしてそろえる。買い足しの量は、.env の鍵でアプリ内課金が有効になっても起動できるよう置く。
const SPEC_CONFIG = '[ocr]\nfree_budget_yen = 60\ntopup_grant_yen = 100\n';

async function main(): Promise<void> {
	// 実行環境の GEMINI_API_KEY を継承すると、OCR機能の有無やスクショの内容が
	// 環境によって変わってしまう(空文字列は「未設定」扱いになる仕様: src/ocr.rs)。
	// backend ではOCR機能を無効に固定し (本物の Gemini を呼ばない)、画面は読み取りの API のモックで
	// 本番と同じ有効な状態にして撮る (shots.ts の login・openPhotoPage)。
	const backend = await startBackend(
		'bp-carnet-spec-',
		{
			GEMINI_API_KEY: '',
			OCR_FREE_BUDGET_YEN: '',
			OCR_TOPUP_GRANT_YEN: '',
			OCR_INPUT_USD_PER_MILLION_TOKENS: '',
			OCR_OUTPUT_USD_PER_MILLION_TOKENS: '',
			OCR_JPY_PER_USD: ''
		},
		{ extraConfig: SPEC_CONFIG }
	);

	let browser: Browser | undefined;
	const cleanup = async () => {
		await browser?.close();
		await backend.stop();
		// 撮影・置換の途中でシグナルを受けた場合、TMP_OUT_DIR が残ったままになりうるので
		// 合わせて片付ける。
		rmSync(TMP_OUT_DIR, { recursive: true, force: true });
		// OUT_DIRをBACKUP_OUT_DIRへ退避した直後、TMP_OUT_DIRをOUT_DIRへrenameする前に
		// シグナルを受けると、OUT_DIRが存在せずBACKUP_OUT_DIRだけが残ったまま終了しうる。
		// その場合は退避物を戻して「前回の完全な生成物を保持する」を保つ
		// (OUT_DIRが既にあれば、置き換えは完了済みとみなしBACKUP_OUT_DIRを削除するだけでよい)。
		if (existsSync(BACKUP_OUT_DIR)) {
			if (!existsSync(OUT_DIR)) {
				renameSync(BACKUP_OUT_DIR, OUT_DIR);
			} else {
				rmSync(BACKUP_OUT_DIR, { recursive: true, force: true });
			}
		}
	};
	// SIGINTだけでなくSIGTERM/SIGHUPでも後始末する。Node.jsの既定動作ではこれらのシグナルは
	// finallyを経由せず即座にプロセスを終了させるため、登録しないと一時起動したbackend
	// プロセスが孤児化し、一時ディレクトリ(BP_CARNET_HOME)も残ってしまう
	// (CIでのタイムアウトkillや親プロセス終了時のシグナル伝播で起こりうる)。
	for (const signal of ['SIGINT', 'SIGTERM', 'SIGHUP'] as const) {
		process.on(signal, () => {
			cleanup().finally(() => process.exit(130));
		});
	}

	try {
		const { baseURL } = backend;

		// startBackend は migrate 完了後 (health 応答後) に返るため、すぐシード投入できる。
		// sqlite3 の CLI ではなく node:sqlite を使うのは、Windows の実機に CLI を入れなくて済むようにするため。
		// backend が起動直後に期限切れセッションを消していることがあるので、ロックは待つ。
		const db = new DatabaseSync(backend.dbPath, { timeout: 5000 });
		try {
			db.exec(readFileSync(SEED_SQL_PATH, 'utf8'));
		} finally {
			db.close();
		}

		// 生成物はまず TMP_OUT_DIR に書き出し、全工程が成功した後でまとめて OUT_DIR に
		// 置き換える(末尾の置き換え処理参照)。途中(撮影・置換)で失敗した場合に、道半ばの
		// 生成物で OUT_DIR を上書きしてしまわないようにするため。OSの一時ディレクトリでは
		// なく OUT_DIR と同じファイルシステム上に置くのは、置き換え時の rename を
		// アトミックに保つため(前回実行の残骸が残っていれば先に消す)。
		rmSync(TMP_OUT_DIR, { recursive: true, force: true });
		const tmpAssetsDir = path.join(TMP_OUT_DIR, 'assets');
		mkdirSync(tmpAssetsDir, { recursive: true });

		// 日時の入力欄の書式は `locale` ではなくブラウザ自体の表示言語で決まるため、起動時に日本語にする
		// (使い方のページの画像と同じ、playwright.config.ts)。
		browser = await chromium.launch({ args: ['--lang=ja-JP'] });

		// id -> viewport順 -> ファイルパス。viewport順は表示テーブルの列順に使う。
		const shotPaths: Record<string, Array<{ viewport: ViewportName; file: string }>> = {};
		for (const [id, entry] of Object.entries(shots)) {
			const viewports = entry.viewports ?? DEFAULT_VIEWPORTS;
			shotPaths[id] = [];
			for (const viewport of viewports) {
				// ショットごとに独立した BrowserContext を使う。1つの page を使い回すと
				// セッションCookieが次のショットに漏れ、「未ログイン状態のはず」のショットが
				// 実は既にログイン済みだった、という混線を招くため。
				// 本番の利用者の環境 (日本語・日本時間) にそろえる (playwright.config.ts と同じ)。
				const context = await browser.newContext({ locale: 'ja-JP', timezoneId: 'Asia/Tokyo' });
				const page = await context.newPage();
				await page.setViewportSize(VIEWPORTS[viewport]);
				const suffix = viewports.length > 1 ? `_${viewport}` : '';
				const outFile = path.join(tmpAssetsDir, `${id}${suffix}.png`);
				await entry.run(page, { baseURL, outFile });
				await context.close();
				shotPaths[id].push({ viewport, file: outFile });
				// 完了後にOUT_DIRへrenameされる前提のパスなので、一時ディレクトリの絶対パスは
				// 表示せず、最終的な配置と同じ相対パスで示す。
				console.log(`[shot] ${id} (${viewport}) -> ${path.relative(TMP_OUT_DIR, outFile)}`);
			}
		}

		const missing: string[] = [];
		const srcFiles = readdirSync(SPEC_SRC_DIR).filter((f) => f.endsWith('.md'));
		for (const file of srcFiles) {
			const template = readFileSync(path.join(SPEC_SRC_DIR, file), 'utf-8');
			const rendered = template.replace(/\{\{shot:([a-zA-Z0-9_]+)\}\}/g, (full, key) => {
				if (!(key in shotPaths)) {
					missing.push(`${file}: ${full}`);
					return full;
				}
				const entries = shotPaths[key];
				if (entries.length === 1) {
					// rename後もディレクトリ構造ごと移動するだけなので、TMP_OUT_DIR基準の
					// 相対パスは最終的なOUT_DIR配下でもそのまま通用する。
					const rel = path.relative(TMP_OUT_DIR, entries[0].file);
					return `![${key}](${rel})`;
				}
				// 複数viewport -> 横並び比較表(GFMテーブル)にする。
				const header = `| ${entries.map((e) => VIEWPORT_LABELS[e.viewport]).join(' | ')} |`;
				const sep = `|${entries.map(() => ' --- ').join('|')}|`;
				const row = `| ${entries
					.map((e) => `![${key}_${e.viewport}](${path.relative(TMP_OUT_DIR, e.file)})`)
					.join(' | ')} |`;
				return `${header}\n${sep}\n${row}`;
			});
			writeFileSync(path.join(TMP_OUT_DIR, file), rendered);
		}

		if (missing.length > 0) {
			throw new Error(`未解決のプレースホルダーがあります: ${missing.join(', ')}`);
		}

		// ここまで全て成功した場合のみ OUT_DIR を置き換える。TMP_OUT_DIR/OUT_DIR は同じ
		// ファイルシステム上にあるので rename はアトミック。まず現行の OUT_DIR を
		// BACKUP_OUT_DIR へ退避してから TMP_OUT_DIR を OUT_DIR に据える2段階にすることで、
		// 万一 rename 自体が失敗しても rmSync(OUT_DIR) 直後ではなく退避物からの復元が
		// できるようにする(「失敗時は前回の完全な生成物を保持する」という意図を守るため)。
		rmSync(BACKUP_OUT_DIR, { recursive: true, force: true });
		mkdirSync(path.dirname(OUT_DIR), { recursive: true });
		const hadPreviousOutput = existsSync(OUT_DIR);
		if (hadPreviousOutput) {
			renameSync(OUT_DIR, BACKUP_OUT_DIR);
		}
		try {
			renameSync(TMP_OUT_DIR, OUT_DIR);
		} catch (err) {
			// 新生成物への切り替えに失敗したら、退避しておいた前回分を戻す。
			if (hadPreviousOutput) {
				renameSync(BACKUP_OUT_DIR, OUT_DIR);
			}
			throw err;
		}
		if (hadPreviousOutput) {
			rmSync(BACKUP_OUT_DIR, { recursive: true, force: true });
		}

		console.log(`生成しました: ${path.relative(REPO_ROOT, OUT_DIR)}/ (${srcFiles.length}ファイル)`);
	} finally {
		await cleanup();
	}
}

main().catch((err) => {
	console.error(err);
	process.exit(1);
});
