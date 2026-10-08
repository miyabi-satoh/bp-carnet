// `just api-types-check` から呼ばれる、生成物の再生成漏れ検査。
//
// openapi.json と src/lib/api/schema.d.ts はコミット対象だが、Rust側のAPIを変更したあと
// `just api-types` を忘れると古いままマージされてしまう。それを CI で落とすための検査。
//
// `fmt-check` と同様、検査は作業ツリーを書き換えない。生成物は一時ディレクトリに出して
// コミット済みのファイルとバイト比較する。
//
// 注意: `cargo run -- --openapi` は target/debug/bp-carnet を作り直すため、
// `just dev-backend` を常駐させたまま実行するとその常駐サーバーが落ちる。
//
// ADR: シェルスクリプトではなく Node で書く。justfile の windows-shell が cmd.exe のため。

import { execFileSync } from 'node:child_process';
import { mkdtempSync, rmSync, readFileSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { REPO_ROOT } from './repo-paths.ts';

const frontendDir = join(REPO_ROOT, 'frontend');
const tmp = mkdtempSync(join(tmpdir(), 'bp-carnet-api-types-'));
// try/finally ではなく exit イベントで消す。process.exit() は finally を実行しないため
process.on('exit', () => rmSync(tmp, { recursive: true, force: true }));

/**
 * 生成コマンドを実行する。失敗したら、コマンドと stderr を出して終了する
 * (握り潰すと「一致しない」のか「生成できなかった」のか切り分けられない)。
 */
function generate(file: string, args: string[], cwd: string): Buffer {
	try {
		return execFileSync(file, args, {
			cwd,
			stdio: ['ignore', 'pipe', 'pipe'],
			maxBuffer: 64 * 1024 * 1024
		});
	} catch (error) {
		const stderr = (error as { stderr?: Buffer }).stderr?.toString() ?? '';
		console.error(`生成に失敗しました: ${[file, ...args].join(' ')}`);
		if (stderr.trim()) console.error(stderr.trimEnd());
		process.exit(1);
	}
}

/** コミット済みファイルと生成物を比較し、一致しなければそのパスを返す */
function staleOrNull(path: string, generated: Buffer): string | null {
	return readFileSync(join(REPO_ROOT, path)).equals(generated) ? null : path;
}

// openapi.json は `bp-carnet --openapi > openapi.json` と同じ経路で作る
// (justfile の `openapi` レシピと揃える)。
const openapi = generate('cargo', ['run', '--quiet', '--', '--openapi'], REPO_ROOT);
const tmpOpenapi = join(tmp, 'openapi.json');
writeFileSync(tmpOpenapi, openapi);

// schema.d.ts は「今のRustコードから作った openapi」を入力に生成する。
// コミット済みの openapi.json を使うと、両方が揃って古い場合に検査をすり抜ける。
//
// pnpm 経由ではなく CLI の実体を node で直接叩く。Windows の `pnpm`・
// `node_modules/.bin/*` は .cmd で、execFileSync では shell 無しに実行できないため。
const tmpSchema = join(tmp, 'schema.d.ts');
const cli = join(frontendDir, 'node_modules', 'openapi-typescript', 'bin', 'cli.js');
generate(process.execPath, [cli, tmpOpenapi, '-o', tmpSchema], frontendDir);

const stale = [
	staleOrNull('openapi.json', openapi),
	staleOrNull('frontend/src/lib/api/schema.d.ts', readFileSync(tmpSchema))
].filter((path): path is string => path !== null);

if (stale.length > 0) {
	console.error(`生成物がコードと一致しません: ${stale.join(', ')}`);
	console.error('`just api-types` を実行して、生成物をコミットしてください。');
	process.exit(1);
}
