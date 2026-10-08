// 抽出したライセンスの情報を、画面 (/licenses) に出す形に整える (→ docs/third-party-licenses.md)。
// 画面の分 (third-party-licenses.ts) と iPhone アプリの分 (generate-ios-licenses.ts) が共通で使う。
//
// 抽出の結果は元データで、そのままは見せない。ここで次のように整える。
// - 1パッケージ1件にする。同じ名前の違う版は1件にまとめ、版を並べる。
// - 同じ条文は1つにまとめ、パッケージからは番号で指す (画面では各パッケージの下にそのまま出す)。
// - 見せられない形の本文が残っていたら、書き出さずに止める。

export interface LicenseText {
	/** SPDX の識別子 (例: `MIT`)。 */
	id: string;
	/** 見出しに出す名前。 */
	name: string;
	/** 著作権表示を含む本文。 */
	text: string;
}

/** 抽出した1つの版。 */
export interface ExtractedPackage {
	name: string;
	version: string;
	/** 宣言しているライセンスの式 (例: `MIT OR Apache-2.0`)。 */
	license: string;
	/** ソースの置き場所 (http(s) のみ)。 */
	repository: string | null;
	/** このパッケージに当たる本文。パッケージが同梱している条文を全部 (1つの本文にファイル名を挟んで並べる)。 */
	texts: LicenseText[];
}

export interface DisplayPackage {
	name: string;
	versions: string[];
	/** 版ごとに式が違えば ` / ` で並べる。 */
	license: string;
	repository: string | null;
	/** `LicenseDisplay.texts` の添え字。 */
	texts: number[];
}

/** 画面が読む形。 */
export interface LicenseDisplay {
	texts: LicenseText[];
	/** 名前の順。 */
	packages: DisplayPackage[];
}

// SPDX のひな形のまま残った本文の印。Apache-2.0 の末尾の適用例 (`[yyyy] [name of copyright owner]`) は
// 条文の一部なので含めない。
const PLACEHOLDER = /<year>|<owner>|<copyright holders>/;

/** 抽出した版の一覧を、画面に出す形に整える。見せられない本文があれば、まとめて挙げて止める。 */
export function buildLicenseDisplay(extracted: ExtractedPackage[]): LicenseDisplay {
	const problems: string[] = [];
	for (const pkg of extracted) {
		const key = `${pkg.name}@${pkg.version}`;
		// 式が空だと、画面の行のライセンスの欄が黙って空になる。
		if (pkg.license.trim() === '') problems.push(`${key}: ライセンスの式が無い`);
		if (pkg.texts.length === 0 || pkg.texts.some((t) => t.text.trim() === '')) {
			problems.push(`${key}: 本文が無い`);
		}
		for (const t of pkg.texts) {
			if (PLACEHOLDER.test(t.text)) problems.push(`${key}: 著作権者が空欄のひな形 (${t.id})`);
		}
	}
	if (problems.length > 0) {
		throw new Error(`見せられない本文がある。条文を確かめて直す:\n${problems.join('\n')}`);
	}

	const texts: LicenseText[] = [];
	const textIndex = new Map<string, number>();
	const indexOf = (t: LicenseText) => {
		const key = `${t.id}\0${t.text}`;
		let index = textIndex.get(key);
		if (index === undefined) {
			index = texts.length;
			textIndex.set(key, index);
			texts.push(t);
		}
		return index;
	};

	const byName = new Map<string, ExtractedPackage[]>();
	for (const pkg of extracted) byName.set(pkg.name, [...(byName.get(pkg.name) ?? []), pkg]);

	const packages = [...byName.entries()]
		.sort(([a], [b]) => compare(a.toLowerCase(), b.toLowerCase()) || compare(a, b))
		.map(([name, versions]) => {
			versions.sort((a, b) => compareVersions(a.version, b.version));
			return {
				name,
				versions: versions.map((v) => v.version),
				license: unique(versions.map((v) => v.license)).join(' / '),
				repository: versions.find((v) => v.repository !== null)?.repository ?? null,
				texts: unique(versions.flatMap((v) => v.texts.map(indexOf)))
			};
		});
	return { texts, packages };
}

export function unique<T>(items: T[]): T[] {
	return [...new Set(items)];
}

/** 並びを OS やロケールに左右させないため、文字コードの順で比べる。 */
function compare(a: string, b: string): number {
	return a < b ? -1 : a > b ? 1 : 0;
}

/** `0.9.4` と `0.10.1` を数の大きさで比べる。数でない部分は文字として比べる。 */
function compareVersions(a: string, b: string): number {
	const pa = a.split(/[.+-]/);
	const pb = b.split(/[.+-]/);
	for (let i = 0; i < Math.max(pa.length, pb.length); i++) {
		const [x, y] = [pa[i] ?? '', pb[i] ?? ''];
		const [nx, ny] = [Number(x), Number(y)];
		const diff =
			x !== '' && y !== '' && !Number.isNaN(nx) && !Number.isNaN(ny) ? nx - ny : compare(x, y);
		if (diff !== 0) return diff;
	}
	return 0;
}

// 配ってよいライセンスの一覧。ここに無いものが入ったら作るのを止め、中身を確かめてから足す。
// 画面の分と iPhone アプリの分で共通に使う。
// - MPL-2.0 (@capgo/capacitor-printer): 手を加えずに配るなら、受け取る人にソースの入手先を知らせれば足りる。
//   画面は行を開くとソースの置き場所を出す。
export const ACCEPTED = [
	'0BSD',
	'Apache-2.0',
	'BSD-2-Clause',
	'BSD-3-Clause',
	'ISC',
	'MIT',
	'MPL-2.0',
	'OFL-1.1',
	'Unlicense'
];

/**
 * 配ってよいライセンスか。
 *
 * SPDX の式 (`(MIT OR Apache-2.0)`・`Apache-2.0 AND MIT`) は項に分けて見る。
 * `OR` はどれか1つ、`AND` は全部が `ACCEPTED` に要る。`AND` が `OR` より強く結び付く。
 * 括弧の入れ子や `WITH` の例外は読まないので、そのときは通さず止めて人に確かめさせる。
 * 括弧を外して読むと、式の意味が変わるため。
 */
export function isAccepted(expression: string): boolean {
	if (/\bWITH\b/i.test(expression)) return false;
	// 式の全体を囲む1組の括弧だけは外してよい。それ以外に括弧が残れば入れ子なので読まない。
	const terms = expression.trim().replace(/^\(([^()]*)\)$/, '$1');
	if (/[()]/.test(terms)) return false;
	const branches = terms
		.split(/\bOR\b/i)
		.map((branch) => branch.split(/\bAND\b/i).map((term) => term.trim()));
	// `MIT AND OR GPL-3.0` のような崩れた式は、残った枝だけで通さない。
	if (branches.flat().some((term) => term === '')) return false;
	// `X+` は「X かそれより後の版」で、受け取る側が X を選べるので X として見る。
	return branches.some((branch) =>
		branch.every((term) => ACCEPTED.includes(term.replace(/\+$/, '')))
	);
}

// 二重ライセンスのパッケージは `LICENSE-MIT`・`LICENSE-APACHE` のように本文を分けて持つ。
const LICENSE_FILE = /^(licen[cs]e|copying)([-._].*)?$/i;
// `license.js` のような同じ名前のコードを本文と間違えないため。
// `LICENSE.spdx` は SPDX のメタデータで、条文ではない。
const NOT_LICENSE_TEXT = /\.(js|mjs|cjs|ts|tsx|json|map|ya?ml|spdx|swift)$/i;

export function isLicenseFileName(name: string): boolean {
	return LICENSE_FILE.test(name) && !NOT_LICENSE_TEXT.test(name);
}

/**
 * 本文のファイルを並べて1つの本文にする。どちらか一方を選ぶと嘘になるので、持っている本文を全部並べる。
 * 2つ以上あるときは、どの条文かが分かるようファイル名を挟む。
 */
export function joinLicenseFiles(files: { name: string; text: string }[]): string {
	return files
		.map(({ name, text }) => (files.length > 1 ? `${name}\n\n${text}` : text))
		.join('\n\n');
}

// npm の `repository` の省略形が指す先。bitbucket だけ `.com` ではない。
const SHORTHAND_HOSTS: Record<string, string> = {
	github: 'github.com',
	gitlab: 'gitlab.com',
	bitbucket: 'bitbucket.org'
};

/**
 * ソースの置き場所の URL。分からなければ `null`。
 *
 * npm の `repository` は `user/repo`・`github:user/repo`・`git@github.com:user/repo.git` のような
 * 書き方も許すので、そのままリンクにすると画面の中の相対パスとして解釈されてしまう。
 */
export function repositoryUrl(repository: unknown): string | null {
	const raw =
		typeof repository === 'string'
			? repository
			: typeof repository === 'object' && repository !== null && 'url' in repository
				? String(repository.url)
				: null;
	if (raw === null) return null;
	const url = raw
		.replace(/^git\+/, '')
		.replace(/\.git$/, '')
		.replace(/^git@([^:]+):/, 'https://$1/')
		.replace(/^(git|ssh):\/\/(git@)?/, 'https://')
		.replace(
			/^(github|gitlab|bitbucket):/,
			(_, host: string) => `https://${SHORTHAND_HOSTS[host]}/`
		)
		// ホストを書かない書き方は GitHub を指す (npm の決まり)。
		.replace(/^(?![a-z]+:\/\/)([\w.-]+\/[\w.-]+)$/, 'https://github.com/$1');
	return /^https?:\/\//.test(url) ? url : null;
}
