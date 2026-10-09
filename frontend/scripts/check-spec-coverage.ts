// `just spec-coverage <dir>` から呼ばれる、画面ごとの仕様書 (環境変数 SPEC_SRC_DIR のフォルダ) の書き漏れ検査。
//
// 仕様書はコードのようにコンパイラに、e2e のようにCIに守られていないため、
// 新しい画面を足してもdocsを書き忘れて気づかれないまま実装だけが進みがち。
// この検査は「全ページにspecを書け」を強制するのではなく、「未記載であることが
// 記録され、気づける」状態を保つためのもの。ROUTE_STATUS に無いルートが増えたら
// 検査を落とし、分類を追記するよう促す。

import { readFileSync, readdirSync, statSync } from 'node:fs';
import { join } from 'node:path';
import { REPO_ROOT } from './repo-paths.ts';

const routesDir = join(REPO_ROOT, 'frontend/src/routes');
const specDir = process.env.SPEC_SRC_DIR;
if (!specDir) {
	throw new Error(
		'SPEC_SRC_DIR (仕様書の原稿のフォルダ) を指定してください (`just spec-coverage <dir>`)'
	);
}

type RouteStatus =
	| { documented: string /* 仕様書のフォルダの中のファイル名 */ }
	| { gap: string /* まだ書かれていない理由・状況 */ };

// 新しいルートを足したら、ここに1行足すこと (`just spec-coverage <dir>` が無いルートを検出する)。
const ROUTE_STATUS: Record<string, RouteStatus> = {
	'/': { documented: '02-home.md' },
	'/admin/users': { documented: '05-admin-users.md' },
	'/admin/users/[id]': { documented: '05-admin-users.md' },
	'/change-email': { documented: '04-settings.md' },
	'/help': { documented: '06-help.md' },
	'/help/[slug]': { documented: '06-help.md' },
	'/licenses': { gap: '仕様書にまだ書いていない' },
	'/login': { documented: '01-login.md' },
	'/payments/ocr-topup/result': { documented: '07-record-photo.md' },
	'/pricing': { documented: '07-record-photo.md' },
	'/privacy': { documented: '01-login.md' },
	'/record/photo': { documented: '07-record-photo.md' },
	'/report': { documented: '03-print-report.md' },
	'/reset-password': { documented: '01-login.md' },
	'/settings': { documented: '04-settings.md' },
	'/settings/change-email': { documented: '04-settings.md' },
	'/settings/change-password': { documented: '04-settings.md' },
	'/settings/import': { documented: '04-settings.md' },
	'/signup': { documented: '01-login.md' },
	'/terms': { documented: '01-login.md' },
	'/verify-email': { documented: '01-login.md' }
};

/** `frontend/src/routes` 配下の `+page.svelte` から、実際のURLパスを集める。 */
function collectRoutePaths(dir: string, prefix = ''): string[] {
	const paths: string[] = [];
	for (const entry of readdirSync(dir)) {
		const full = join(dir, entry);
		if (statSync(full).isDirectory()) {
			// ルートグループ `(group)` はURLに現れないので、prefixに足さない。
			const nextPrefix =
				entry.startsWith('(') && entry.endsWith(')') ? prefix : `${prefix}/${entry}`;
			paths.push(...collectRoutePaths(full, nextPrefix));
		} else if (entry === '+page.svelte') {
			paths.push(prefix === '' ? '/' : prefix);
		}
	}
	return paths;
}

const actualRoutes = new Set(collectRoutePaths(routesDir));
const knownRoutes = new Set(Object.keys(ROUTE_STATUS));

const missing = [...actualRoutes].filter((route) => !knownRoutes.has(route)).sort();
const stale = [...knownRoutes].filter((route) => !actualRoutes.has(route)).sort();

// 「記載済み」と申告したファイルは、実在だけ確かめる (パス文字列の表記揺れ
// (`{id}`/`[id]`/`<slug>` 等) までは追わない。過検知の方が実害が大きいため)。
const brokenReferences = Object.entries(ROUTE_STATUS)
	.filter((entry): entry is [string, { documented: string }] => 'documented' in entry[1])
	.filter(([, status]) => {
		try {
			readFileSync(join(specDir, status.documented));
			return false;
		} catch {
			return true;
		}
	});

let ok = true;

if (missing.length > 0) {
	ok = false;
	console.error(
		'ROUTE_STATUS に無いルートがあります (check-spec-coverage.ts に分類を追記してください):'
	);
	for (const route of missing) console.error(`  ${route}`);
}

if (stale.length > 0) {
	ok = false;
	console.error('ROUTE_STATUS にあるが、もう存在しないルートです (削除してください):');
	for (const route of stale) console.error(`  ${route}`);
}

if (brokenReferences.length > 0) {
	ok = false;
	console.error('documented として申告されたファイルが仕様書のフォルダにありません:');
	for (const [route, status] of brokenReferences)
		console.error(`  ${route} -> ${status.documented}`);
}

if (!ok) process.exit(1);

const gaps = Object.entries(ROUTE_STATUS).filter(
	(entry): entry is [string, { gap: string }] => 'gap' in entry[1]
);
if (gaps.length > 0) {
	console.log(`仕様書に未記載のルート (${gaps.length}件、参考情報。検査は失敗させない):`);
	for (const [route, status] of gaps) console.log(`  ${route}: ${status.gap}`);
}
