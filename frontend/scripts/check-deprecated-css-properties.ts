// `just lint` (pnpm run lint) から呼ばれる、非推奨 CSS プロパティの検査。対象は2箇所:
//
//   1. 実際の .css ファイル (layout.css 等)。
//   2. Tailwind の任意プロパティ (`[prop:value]`)。Svelte 側の文字列 (クラス名) でしかなく
//      CSS として解釈されないため、stylelint はもちろん Tailwind 自身も中身を検査しない。
//
// stylelint の property-no-deprecated (内蔵リスト) をベースに使うが、内蔵リストは
// page-break-*・grid-gap・clip 等の一部しかカバーせず、color-adjust を含まない。
// 取りこぼしを埋めるため、内蔵リストに無い既知の非推奨プロパティを
// SUPPLEMENTARY_DEPRECATED で補う。

import { readdirSync, readFileSync } from 'node:fs';
import { extname, join } from 'node:path';
import stylelint from 'stylelint';
import { REPO_ROOT } from './repo-paths.ts';

const SRC_DIR = join(REPO_ROOT, 'frontend', 'src');

/** stylelint の property-no-deprecated にまだ収録されていない既知の非推奨プロパティ。
 * 新しく見つかったらここに足す。 */
const SUPPLEMENTARY_DEPRECATED: Record<string, string> = {
	'color-adjust': 'print-color-adjust'
};

type Problem = { file: string; property: string; message: string };

function isExcludedPath(path: string): boolean {
	// shadcn-svelte のレジストリ由来コンポーネントはプロジェクト側の改変対象外 (AGENTS.md)。
	return path.includes(join('src', 'lib', 'components', 'ui'));
}

function collectFiles(dir: string, ext: string): string[] {
	const files: string[] = [];
	for (const entry of readdirSync(dir, { withFileTypes: true, recursive: true })) {
		if (!entry.isFile() || extname(entry.name) !== ext) continue;
		const path = join(entry.parentPath, entry.name);
		if (!isExcludedPath(path)) files.push(path);
	}
	return files;
}

// Tailwind 任意プロパティ構文: `[property-name:value]`。プロパティ名は CSS のカスタム
// プロパティ以外は英小文字とハイフンのみなのでこの範囲に絞る。Tailwind のクラス文字列は
// .svelte のマークアップにしか出てこない (.ts まで含めると schema.d.ts の TypeScript
// インデックスシグネチャ `[name: string]: unknown` 等を誤検出する)。
const ARBITRARY_PROPERTY_RE = /\[([a-z-]+):[^\]]+\]/g;
const arbitraryOccurrences: { file: string; property: string }[] = [];
for (const file of collectFiles(SRC_DIR, '.svelte')) {
	const text = readFileSync(file, 'utf8');
	for (const match of text.matchAll(ARBITRARY_PROPERTY_RE)) {
		arbitraryOccurrences.push({ file, property: match[1] });
	}
}
const uniqueArbitraryProps = [...new Set(arbitraryOccurrences.map((o) => o.property))];

const cssFiles = collectFiles(SRC_DIR, '.css');

async function checkStylelintBuiltinList(): Promise<Problem[]> {
	const problems: Problem[] = [];

	if (uniqueArbitraryProps.length > 0) {
		// 1プロパティ1行の疑似CSSにして、警告行番号からそのままプロパティ名を引けるようにする。
		const pseudoCss = uniqueArbitraryProps.map((p) => `.x { ${p}: unset; }`).join('\n');
		const result = await stylelint.lint({
			code: pseudoCss,
			config: { rules: { 'property-no-deprecated': true } }
		});
		for (const warning of result.results.flatMap((r) => r.warnings)) {
			const property = uniqueArbitraryProps[warning.line - 1];
			for (const o of arbitraryOccurrences.filter((o) => o.property === property)) {
				problems.push({ file: o.file, property, message: warning.text });
			}
		}
	}

	if (cssFiles.length > 0) {
		const result = await stylelint.lint({
			files: cssFiles,
			config: { rules: { 'property-no-deprecated': true } }
		});
		for (const r of result.results) {
			for (const warning of r.warnings) {
				problems.push({
					file: r.source ?? '(unknown)',
					property: warning.text,
					message: warning.text
				});
			}
		}
	}

	return problems;
}

function checkSupplementaryList(): Problem[] {
	const problems: Problem[] = [];

	for (const o of arbitraryOccurrences) {
		const replacement = SUPPLEMENTARY_DEPRECATED[o.property];
		if (replacement) {
			problems.push({
				file: o.file,
				property: o.property,
				message: `非推奨プロパティ "${o.property}" (→ "${replacement}")`
			});
		}
	}

	for (const file of cssFiles) {
		// CSS のコメントは常に /* ... */ (行コメントは無い) なので、先に取り除いてから
		// 検査する。取り除かないと `/* color-adjust: ... について */` のような、
		// このプロパティ自体を説明するコメントにも誤検知する。
		const text = readFileSync(file, 'utf8').replace(/\/\*[\s\S]*?\*\//g, '');
		for (const [prop, replacement] of Object.entries(SUPPLEMENTARY_DEPRECATED)) {
			// プロパティ宣言としての出現のみ対象 (直前が識別子文字でない "prop:" の形を要求)。
			const re = new RegExp(`(?<![\\w-])${prop}\\s*:`, 'g');
			if (re.test(text)) {
				problems.push({
					file,
					property: prop,
					message: `非推奨プロパティ "${prop}" (→ "${replacement}")`
				});
			}
		}
	}

	return problems;
}

const problems = [...(await checkStylelintBuiltinList()), ...checkSupplementaryList()];

if (problems.length > 0) {
	console.error('非推奨 CSS プロパティが見つかりました:');
	for (const p of problems) {
		console.error(`  [${p.property}] ${p.message}`);
		console.error(`    ${p.file}`);
	}
	process.exit(1);
}

console.log(
	`CSS ${cssFiles.length}件・Tailwind任意プロパティ ${uniqueArbitraryProps.length}種類 (使用箇所 ${arbitraryOccurrences.length}件) を確認、問題ありません。`
);
