// `just e2e-local` (→ `just ci`) から呼ばれる。使い捨ての backend を立てて e2e を流し、片付ける。
//
// 起動済みのサーバーに当てる `just e2e` と違い、開発用 DB を使わない。e2e が作ったものは
// 後片付けするが、途中で落ちたときに開発用 DB を汚さないで済む。
// 管理者は、backend 起動後にテスト用 DB へ直接入れる (→ seedAdmin)。
// 記録やユーザーは各テストが自分で作る前提 (→ frontend/e2e/api-helpers.ts)。
// メールは backend と一緒に立てる SMTP の受け口で受け取り、テストはそれを読む (→ backend-process.ts・e2e/mail-helpers.ts)。
// `--help-shots` を付けると、使い方のページのシナリオだけを流し、手順の画像を撮る (`just help-shots`、→ frontend/e2e/help-shots.ts)。

import { spawn, type ChildProcess, type SpawnOptions } from 'node:child_process';
import path from 'node:path';
import { DatabaseSync } from 'node:sqlite';
import { KEEP_DATA } from '../e2e/kept-data.ts';
import { startBackend } from './backend-process.ts';

const ADMIN_USERNAME = 'admin';
const ADMIN_PASSWORD = 'password';
const ADMIN_PASSWORD_HASH =
	'$argon2id$v=19$m=19456,t=2,p=1$7ve40YOoyZyokwe3oSoRVQ$s0pznuPy5oXx7tDx9ECsB88rj+CdXzf0ZJ7TdlqFL6w';

/** 中断されたときの終了コード (SIGINT で終わったプロセスの慣習)。 */
const INTERRUPTED_EXIT_CODE = 130;
/** 止める合図を送ってから、強制的に止めて片付けへ進むまで待つ時間。Playwright が後始末を終えるのに足りる長さ。 */
const PLAYWRIGHT_STOP_TIMEOUT_MS = 10_000;

const IS_WINDOWS = process.platform === 'win32';
const HELP_SHOTS = process.argv.includes('--help-shots');

let interrupted = false;
/** 実行中の Playwright を止める。`runPlaywright` の間だけ入る。 */
let stopRunningPlaywright: (() => void) | undefined;

/**
 * 子と、その下の pnpm・Playwright をまとめて止める。
 * Windows では shell 経由の子が cmd.exe なので、`kill` では下が残る。`taskkill /T` で木ごと止め、
 * それが使えなければ子だけでも止める。それ以外では、子を別のプロセスグループで起動しているので、
 * グループへ送る。
 */
function signalTree(child: ChildProcess, force: boolean): void {
	const pid = child.pid;
	if (pid === undefined) return;
	if (IS_WINDOWS) {
		const killer = spawn('taskkill', ['/pid', String(pid), '/T', '/F'], { stdio: 'ignore' });
		killer.on('error', () => child.kill());
		killer.on('exit', (code) => {
			if (code !== 0) child.kill();
		});
		return;
	}
	try {
		process.kill(-pid, force ? 'SIGKILL' : 'SIGINT');
	} catch {
		// グループが既に無い (終わった後) なら何もしなくてよい。
	}
}

/**
 * Playwright を、使い捨ての backend と管理者で流し、終了コードを返す。中断されたら 130 を返す。
 * `pnpm run test:e2e` を使わないのは、あちらが毎回 `playwright install` を走らせるため。
 * push のたびに走らせない。ブラウザは `just install` で入れる。
 */
function runPlaywright(baseURL: string, storageState: string, mailDir: string): Promise<number> {
	return new Promise((resolve, reject) => {
		const options: SpawnOptions = {
			stdio: 'inherit',
			// 別のプロセスグループにして、中断時にグループごと止められるようにする (→ signalTree)。
			// 端末の Ctrl-C は親のグループにしか届かないが、親が受けて転送する。
			detached: !IS_WINDOWS,
			env: {
				...process.env,
				E2E_BASE_URL: baseURL,
				E2E_ADMIN_USER: ADMIN_USERNAME,
				E2E_ADMIN_PASSWORD: ADMIN_PASSWORD,
				// 手動の `just e2e` とログイン状態のファイルを取り合わないよう、使い捨ての場所に置く。
				E2E_ADMIN_STORAGE_STATE: storageState,
				E2E_MAIL_DIR: mailDir,
				// justfile から環境変数を渡すと Windows の cmd.exe で書き方が変わるため、引数で受ける。
				...(HELP_SHOTS ? { E2E_HELP_SHOTS: '1' } : {})
			}
		};
		const args = ['exec', 'playwright', 'test', ...(HELP_SHOTS ? ['e2e/help.e2e.ts'] : [])];
		// Windows の pnpm は pnpm.cmd なので、shell を通さないと解決できない。shell を通すときに
		// 引数を配列で渡すと Node が DEP0190 で警告するので、1本のコマンドにする (引数は固定)。
		const child = IS_WINDOWS
			? spawn(`pnpm ${args.join(' ')}`, { ...options, shell: true })
			: spawn('pnpm', args, options);

		let settled = false;
		const settle = (result: () => void) => {
			if (settled) return;
			settled = true;
			stopRunningPlaywright = undefined;
			result();
		};
		child.on('error', (err) => settle(() => reject(err)));
		// 中断した後は、子が 0 で終わっても成功にしない。
		child.on('exit', (code) =>
			settle(() => resolve(interrupted ? INTERRUPTED_EXIT_CODE : (code ?? 1)))
		);

		let stopping = false;
		stopRunningPlaywright = () => {
			if (stopping) return;
			stopping = true;
			signalTree(child, false);
			// 合図を無視されても、強制的に止めて片付けへ進む。子が残っても待ち続けない。
			setTimeout(() => {
				signalTree(child, true);
				settle(() => resolve(INTERRUPTED_EXIT_CODE));
			}, PLAYWRIGHT_STOP_TIMEOUT_MS).unref();
		};
		// spawn の直前に中断されていたら、起動した直後に止める。
		if (interrupted) stopRunningPlaywright();
	});
}

function seedAdmin(dbPath: string): void {
	const db = new DatabaseSync(dbPath, { timeout: 5000 });
	try {
		db.prepare(
			"INSERT INTO users (username, password_hash, role, email_verified) VALUES (?, ?, 'admin', 1)"
		).run(ADMIN_USERNAME, ADMIN_PASSWORD_HASH);
	} finally {
		db.close();
	}
}

async function main(): Promise<number> {
	// 使い捨ての backend は終わると DB ごと消えるため、残しても後から確かめられない。
	if (KEEP_DATA) {
		console.error(
			'E2E_KEEP_DATA は just e2e-local では使えない。起動済みの環境に当てる just e2e で使う。'
		);
		return 1;
	}

	// 親まで落ちると finally の片付けが走らず、一時 BP_CARNET_HOME が残る。親は落とさずに、
	// Playwright を止めてから片付けへ進める。何度受けても止める処理は1回だけ。
	// Ctrl-C だけでなく、kill (SIGTERM) や端末を閉じたとき (SIGHUP) も同じにする。
	for (const signal of ['SIGINT', 'SIGTERM', 'SIGHUP'] as const) {
		process.on(signal, () => {
			interrupted = true;
			stopRunningPlaywright?.();
		});
	}

	return await runWithBackend();
}

/**
 * 写真の読み取り (`E2E_OCR=1`)・アプリ内課金 (.env の鍵で有効になりうる) を有効にして起動するのに要る値。
 * 実際の価格ではない。無料枠は無制限にし、上限の表示は e2e 側で応答を差し替えて確かめる。
 */
const E2E_OCR_CONFIG = `[ocr]
free_budget_yen = -1
topup_grant_yen = 100

[ocr.pricing]
input_usd_per_million_tokens = 1.0
output_usd_per_million_tokens = 4.0
jpy_per_usd = 100.0
`;

async function runWithBackend(): Promise<number> {
	// 写真の読み取りは Gemini の利用回数を消費するため、明示したとき (`E2E_OCR=1`) だけ有効にする。
	// LINE・Google・Apple ログインは、public_url があるので .env の認証情報で有効になってしまう。
	// 使わない構成 (開発・テストの既定) にそろえる。出す構成は e2e 側で応答を差し替えて確かめる。
	// 空文字列は未設定扱い。backend はリポジトリの .env も読むが、既にある環境変数は上書きしない。
	const backend = await startBackend(
		'bp-carnet-e2e-',
		{
			...(process.env.E2E_OCR === '1' ? {} : { GEMINI_API_KEY: '' }),
			LINE_LOGIN_CHANNEL_ID: '',
			LINE_LOGIN_CHANNEL_SECRET: '',
			GOOGLE_OAUTH_CLIENT_ID: '',
			GOOGLE_OAUTH_CLIENT_SECRET: '',
			APPLE_TEAM_ID: '',
			APPLE_KEY_ID: '',
			APPLE_PRIVATE_KEY: '',
			// 下の E2E_OCR_CONFIG を使わせる (.env の値は開発用で、E2E の前提と違いうる)。
			OCR_FREE_BUDGET_YEN: '',
			OCR_TOPUP_GRANT_YEN: '',
			OCR_INPUT_USD_PER_MILLION_TOKENS: '',
			OCR_OUTPUT_USD_PER_MILLION_TOKENS: '',
			OCR_JPY_PER_USD: ''
		},
		{ extraConfig: E2E_OCR_CONFIG }
	);
	try {
		seedAdmin(backend.dbPath);
		// 準備の途中で中断されたら、テストを始めずに片付ける。
		if (interrupted) return INTERRUPTED_EXIT_CODE;
		const storageState = path.join(path.dirname(backend.dbPath), 'e2e-admin.json');
		return await runPlaywright(backend.baseURL, storageState, backend.mailDir);
	} finally {
		await backend.stop();
	}
}

main().then(
	(code) => {
		process.exitCode = code;
	},
	(err) => {
		console.error(err);
		process.exitCode = 1;
	}
);
