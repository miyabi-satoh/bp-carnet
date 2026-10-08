import { execFileSync } from 'node:child_process';
import path from 'node:path';

export const REPO_ROOT = path.resolve(import.meta.dirname, '..', '..');

/**
 * git 管理外の `data/` (開発用の DB・設定・テスト画像・キャッシュ) の場所。git worktree の中からでも、
 * 元の clone の `data/` を指す。worktree ごとに写すと、DB とポートの設定が分かれるため。
 * git が使えなければ、このリポジトリの `data/` にする。
 */
export function dataDir(): string {
	try {
		const commonDir = execFileSync(
			'git',
			['rev-parse', '--path-format=absolute', '--git-common-dir'],
			{ cwd: REPO_ROOT, encoding: 'utf8', stdio: ['ignore', 'pipe', 'ignore'] }
		).trim();
		return path.join(path.dirname(commonDir), 'data');
	} catch {
		return path.join(REPO_ROOT, 'data');
	}
}
