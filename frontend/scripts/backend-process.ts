// 使い捨ての backend (`target/debug/bp-carnet`) を起動・停止する。
// `just spec` (generate-spec.ts) と `just e2e-local` (run-e2e.ts) で共有する。
//
// 一時 BP_CARNET_HOME と空きポートで起動するので、開発用 DB にも、並行して動いている
// 開発サーバーにも影響しない (single_instance のロックも一時ディレクトリに置かれる)。
// 本番で必ず入る設定 (public_url・contact_url・[mail]) も入れる。メールは一緒に立てる受け口 (→ mail-sink.ts) で受ける。
// 呼び出し側は必ず `stop()` を呼ぶこと。

import { spawn, type ChildProcess } from 'node:child_process';
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from 'node:fs';
import net from 'node:net';
import { tmpdir } from 'node:os';
import path from 'node:path';
import { startMailSink, type MailSink } from './mail-sink.ts';
import { REPO_ROOT } from './repo-paths.ts';

const BIN_PATH = path.join(
	REPO_ROOT,
	'target',
	'debug',
	process.platform === 'win32' ? 'bp-carnet.exe' : 'bp-carnet'
);

export interface Backend {
	baseURL: string;
	/** マイグレーション済みの DB ファイル。起動を待ってから返すので、すぐ書き込める。 */
	dbPath: string;
	/** 送ったメールを1通ずつ JSON で置く場所 (→ mail-sink.ts)。`stop()` で消える。 */
	mailDir: string;
	/** 起動からの標準出力・標準エラー。失敗の原因を出すときに使う。 */
	log: () => string;
	/** プロセスの終了を待ってから一時ディレクトリを消す。何度呼んでもよい。 */
	stop: () => Promise<void>;
}

// OSに空きポートを選ばせる。固定ポートだと、既に何かが同じポートで listen していた場合
// waitForPort がその別プロセスの応答を「起動成功」と誤判定してしまう
// (新規プロセス自身はbind失敗で終了しているのに、一時DBへの書き込みが無関係な別プロセスに
// 向かってしまう)。
async function getFreePort(): Promise<number> {
	return new Promise((resolve, reject) => {
		const server = net.createServer();
		server.on('error', reject);
		server.listen(0, '127.0.0.1', () => {
			const address = server.address();
			if (address && typeof address === 'object') {
				const port = address.port;
				server.close(() => resolve(port));
			} else {
				server.close();
				reject(new Error('空きポートの確保に失敗しました'));
			}
		});
	});
}

// 動的ポート確保だけでも大半の競合は防げるが、念のため子プロセスの早期終了(bind失敗等)も
// 監視し、health応答を待たずに検知できるようにする。
async function waitForPort(url: string, timeoutMs: number, proc: ChildProcess): Promise<void> {
	const deadline = Date.now() + timeoutMs;
	let exitInfo: string | null = null;
	proc.once('exit', (code, signal) => {
		exitInfo = `code=${code} signal=${signal}`;
	});
	// 実行ファイルが無い等で起動できないときは exit が来ず error だけが来る。
	proc.once('error', (err) => {
		exitInfo = err.message;
	});
	while (Date.now() < deadline) {
		if (exitInfo !== null) {
			throw new Error(`backendプロセスが起動前に終了しました(${exitInfo})`);
		}
		try {
			const res = await fetch(url);
			if (res.status < 500) return;
		} catch {
			// まだ起動していない。リトライする。
		}
		await new Promise((r) => setTimeout(r, 200));
	}
	throw new Error(`backendが${timeoutMs}ms以内に起動しませんでした: ${url}`);
}

// SIGTERM送信後、プロセスが実際に終了するまで待つ(掴んでいるDBファイルを残したまま
// 一時ディレクトリを削除しないため)。無反応な場合に備えてタイムアウトでSIGKILLする。
async function killAndWait(proc: ChildProcess, timeoutMs = 5000): Promise<void> {
	// 起動できなかった (pid が無い) プロセスには exit が来ないので、待たずに返す。
	if (proc.pid === undefined || proc.exitCode !== null || proc.signalCode !== null) return;
	await new Promise<void>((resolve) => {
		const timer = setTimeout(() => proc.kill('SIGKILL'), timeoutMs);
		proc.once('exit', () => {
			clearTimeout(timer);
			resolve();
		});
		proc.kill('SIGTERM');
	});
}

/**
 * backend を起動し、health が応答するまで待つ。起動に失敗したら、ログを出して片付けてから投げる。
 * `tmpPrefix` は一時 BP_CARNET_HOME の名前の頭 (どのスクリプトの残骸か分かるようにする)。
 * `env` は実行環境の環境変数に上書きして渡す。
 * `extraConfig` は config.toml の末尾にそのまま足す TOML の断片
 * (写真の読み取り・買い足しに要る `[ocr]` を差し込むのに使う)。
 */
export async function startBackend(
	tmpPrefix: string,
	env: Record<string, string> = {},
	{ extraConfig }: { extraConfig?: string } = {}
): Promise<Backend> {
	const port = await getFreePort();
	const home = mkdtempSync(path.join(tmpdir(), tmpPrefix));
	const mailDir = path.join(home, 'mail');
	const baseURL = `http://127.0.0.1:${port}`;
	let mailSink: MailSink | undefined;
	let proc: ChildProcess;
	try {
		mkdirSync(mailDir);
		mailSink = await startMailSink(mailDir);
		writeFileSync(
			path.join(home, 'config.toml'),
			`[server]\nbind = "127.0.0.1"\nport = ${port}\npublic_url = "${baseURL}"\ncontact_url = "https://example.com/contact/"\n\n[log]\nfilter = "info"\noutput = "stdout"\n\n[session]\nsecret = ""\nsecure_cookie = false\nexpiry_days = 14\n\n[mail]\nhost = "127.0.0.1"\nport = ${mailSink.port}\ntls = "none"\nfrom = "BP Carnet <noreply@example.com>"\n\n${extraConfig ?? ''}`
		);
		proc = spawn(BIN_PATH, [], {
			env: { ...process.env, ...env, BP_CARNET_HOME: home },
			stdio: ['ignore', 'pipe', 'pipe']
		});
	} catch (err) {
		// ここで投げると呼び出し側は stop() を持たないので、ここで片付ける。
		await mailSink?.stop();
		rmSync(home, { recursive: true, force: true });
		throw err;
	}
	const sink = mailSink;
	let log = '';
	proc.stdout?.on('data', (d) => (log += d));
	proc.stderr?.on('data', (d) => (log += d));
	// 起動できなかったときの error を拾わないと、未処理のイベントで node ごと落ちて片付けに届かない。
	proc.on('error', (err) => (log += `\n起動できませんでした: ${err.message}\n`));

	// 受け口は2度閉じられないので、2度目以降は1度目の片付けを待つだけにする。
	let stopping: Promise<void> | undefined;
	const stop = () => (stopping ??= stopOnce());
	const stopOnce = async () => {
		await killAndWait(proc);
		await sink.stop();
		try {
			// Windows では終了の直後もウイルス対策ソフト等が DB を掴んでいることがあるので、やり直す。
			rmSync(home, { recursive: true, force: true, maxRetries: 5 });
		} catch (err) {
			// 片付けの失敗で、呼び出し側の結果 (テストの成否など) を覆さない。
			console.warn(`一時ディレクトリを消せませんでした (${home}):`, err);
		}
	};

	// 起動待ちの間は、呼び出し側がまだ片付けを持っていない。Ctrl-C・kill・端末を閉じても残さない。
	const INTERRUPT_SIGNALS = ['SIGINT', 'SIGTERM', 'SIGHUP'] as const;
	const onInterrupt = () => {
		void stop().finally(() => process.exit(130));
	};
	for (const signal of INTERRUPT_SIGNALS) process.once(signal, onInterrupt);

	try {
		// db::migrate() は bind より前に実行されるので、health が通れば DB は書き込める。
		await waitForPort(`${baseURL}/api/v1/health`, 10_000, proc);
	} catch (err) {
		console.error('--- backend log ---\n' + log);
		await stop();
		throw err;
	} finally {
		for (const signal of INTERRUPT_SIGNALS) process.off(signal, onInterrupt);
	}
	return { baseURL, dbPath: path.join(home, 'bp-carnet.db'), mailDir, log: () => log, stop };
}
