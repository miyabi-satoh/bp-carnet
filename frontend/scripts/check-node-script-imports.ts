// `just lint` (pnpm run lint) から呼ばれる、node で直に動かすスクリプトの相対 import の検査。
//
// scripts/*.ts は node の型の除去だけで動かし、node は拡張子の無い相対 import を解決しない。
// e2e/ のファイルは Playwright が解決するので拡張子なしで書くが、shots.ts などから読まれたものだけは
// 拡張子が要る。CI は `just spec` を流さないので、ここで静的に拾う。
// 型だけの import は node が消すので見ない。

import { existsSync, readdirSync, readFileSync } from 'node:fs';
import { dirname, extname, join, relative, resolve } from 'node:path';
import { REPO_ROOT } from './repo-paths.ts';

const SCRIPTS_DIR = join(REPO_ROOT, 'frontend', 'scripts');

// `import ... from '...'`・`export ... from '...'`・`import '...'`・`import('...')`。
// 1つ目の組は `type` の有無 (型だけの import・export を見分ける)。
const IMPORT_RE =
	/(?:^|[;\n])\s*(?:import|export)\s+(type\s+)?(?:[^'";]*?\sfrom\s+)?['"]([^'"]+)['"]|import\(\s*['"]([^'"]+)['"]\s*\)/g;

type Problem = { file: string; specifier: string };

function relativeImports(file: string): string[] {
	const text = readFileSync(file, 'utf8');
	const specifiers: string[] = [];
	for (const match of text.matchAll(IMPORT_RE)) {
		const [, typeOnly, staticSpecifier, dynamicSpecifier] = match;
		if (typeOnly) continue;
		const specifier = staticSpecifier ?? dynamicSpecifier;
		if (specifier.startsWith('.')) specifiers.push(specifier);
	}
	return specifiers;
}

const problems: Problem[] = [];
const pending = readdirSync(SCRIPTS_DIR)
	.filter((name) => extname(name) === '.ts')
	.map((name) => join(SCRIPTS_DIR, name));
const seen = new Set<string>();
while (pending.length > 0) {
	const file = pending.pop() as string;
	if (seen.has(file)) continue;
	seen.add(file);
	for (const specifier of relativeImports(file)) {
		if (extname(specifier) === '') {
			problems.push({ file, specifier });
			continue;
		}
		const target = resolve(dirname(file), specifier);
		if (extname(target) === '.ts' && existsSync(target)) pending.push(target);
	}
}

if (problems.length > 0) {
	console.error(
		'node で動かすスクリプトから読まれるファイルに、拡張子の無い相対 import があります:'
	);
	for (const { file, specifier } of problems) {
		console.error(`  ${relative(REPO_ROOT, file)}: '${specifier}'`);
	}
	process.exit(1);
}
