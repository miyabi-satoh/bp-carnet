// サーバー (Rust) の依存に、ネットワーク越しに使わせるだけで条件が掛かるライセンスが入っていないかを見る
// (`just licenses-check`。→ docs/third-party-licenses.md)。
// サーバーは利用者に配らないのでライセンスの表示は要らないが、AGPL・SSPL はサーバーで動かすだけでソースの公開を求める。

import { execFileSync } from 'node:child_process';
import { REPO_ROOT } from './repo-paths.ts';

// ネットワーク越しの利用で条件が掛かるライセンス。
const NETWORK_COPYLEFT = /^(AGPL|SSPL)/i;

interface Metadata {
	packages: { id: string; name: string; version: string; license: string | null }[];
	workspace_members: string[];
}

const metadata = JSON.parse(
	execFileSync('cargo', ['metadata', '--format-version', '1', '--locked'], {
		cwd: REPO_ROOT,
		encoding: 'utf8',
		maxBuffer: 64 * 1024 * 1024
	})
) as Metadata;

const problems = metadata.packages
	.filter((pkg) => !metadata.workspace_members.includes(pkg.id))
	.filter((pkg) => {
		if (pkg.license === null) return true;
		// `OR` の枝のどれかが当てはまらなければ、そちらを選べる。
		return pkg.license
			.replace(/[()]/g, '')
			.split(/\bOR\b|\//)
			.every((branch) => branch.split(/\bAND\b/).some((t) => NETWORK_COPYLEFT.test(t.trim())));
	})
	.map((pkg) => `${pkg.name}@${pkg.version}: ${pkg.license ?? 'ライセンスの欄が無い'}`);

if (problems.length > 0) {
	console.error(`サーバーの依存のライセンスを確かめる:\n${problems.join('\n')}`);
	process.exit(1);
}
