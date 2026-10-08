// 画面のビルドに同梱した npm のパッケージのライセンス表示を、ビルドの出力に書き出す Vite プラグイン
// (→ docs/third-party-licenses.md)。
// package.json の依存ではなく、実際に chunk に入ったモジュールから集める。lint やテストの道具を表示に混ぜないため。

import { existsSync, readdirSync, readFileSync, realpathSync } from 'node:fs';
import { createRequire } from 'node:module';
import { join, resolve } from 'node:path';
import type { Plugin } from 'vite';
import {
	buildLicenseDisplay,
	isAccepted,
	isLicenseFileName,
	joinLicenseFiles,
	repositoryUrl,
	type ExtractedPackage
} from './license-display.ts';

const NPM_LICENSES_FILE = 'third-party-licenses/npm.json';

const NODE_MODULES = '/node_modules/';

// コードを同梱しているのに、chunk からは辿れないパッケージ。
// - Paraglide は `src/lib/paraglide` に出力するので、モジュールの ID が node_modules の外になる。
//   出力はコンパイラが書いたコードで、そのままバンドルに入る。
// - CSS の `@import` (`src/routes/layout.css`) は @tailwindcss/vite が展開してしまい、
//   Rollup のモジュールにもアセットにも出てこない。中身は配る CSS に入っている。
const EXTRA_PACKAGES = ['@inlang/paraglide-js', 'tailwindcss', 'tw-animate-css', 'shadcn-svelte'];

// Service Worker の実行部分 (`build/workbox-*.js`)。@vite-pwa/sveltekit が workbox-build で別に作るので、
// このプラグインからは見えない。中身は出力の `workbox:<名前>:` の印で確かめた4つで、
// どれも直接の依存ではないので、@vite-pwa/sveltekit → vite-plugin-pwa → workbox-build と辿って探す。
const WORKBOX_PACKAGES = [
	'workbox-core',
	'workbox-precaching',
	'workbox-routing',
	'workbox-strategies'
];

// ビルドの道具がバンドルに差し込む仮想モジュール (ID が `\0` で始まる) と、そのコードを持つパッケージ。
// node_modules の外の ID なので、ここで対応を付けないと数え漏れる。
// 知らない仮想モジュールがバンドルに入ったらビルドを止め、どのパッケージのコードかを確かめてから足す。
// ID は完全に一致したものだけを通す。前方一致にすると、同じ道具の知らないモジュールまで確かめずに通ってしまう。
const VIRTUAL_MODULES: Record<string, (root: string) => string> = {
	'\0vite/preload-helper.js': (root) => viteDir(root),
	// Vite が読み込む Rolldown を、Vite の場所から Node の解決で探す。
	'\0rolldown/runtime.js': (root) => resolvePackage(viteDir(root), 'rolldown')
};

// 本文の中で、配る物に入らない部分が始まる印。そこから後ろは表示しない。
// - Vite の `LICENSE.md` は、Vite 本体の条文の後に、Vite 自身が同梱する依存の条文を2千行ほど続けている。
//   配る物に入るのは Vite 本体のコード (上の preload-helper) だけなので、本体の条文だけを出す。
const TEXT_END: Record<string, string> = {
	vite: '\n# Licenses of bundled dependencies'
};

// 名前が LICENSE で始まらないので拾えないが、本文として足すファイル。
// - Rolldown の `LICENSE` は、一部が外部のライブラリに由来し、その条文は THIRD-PARTY-LICENSE にあると書いている。
const EXTRA_TEXT_FILES: Record<string, string[]> = {
	rolldown: ['THIRD-PARTY-LICENSE']
};

// package.json に置き場所を書いていないパッケージと、確かめた上流のリポジトリ。
// - svelte-toolbelt: package.json に repository も homepage も無い。同梱の LICENSE の著作権表示が、このリポジトリの LICENSE と一致する。
const REPOSITORIES: Record<string, string> = {
	'svelte-toolbelt': 'https://github.com/huntabyte/svelte-toolbelt'
};

// package.json の宣言が、同梱している本文と食い違うパッケージと、本文に合わせた式。
// 版を添えるのは、上流が直したときに上書きを外せるよう、版が変わったらビルドを止めて見直させるため。
// - @capacitor/synapse: package.json は ISC だが、同梱の LICENSE.md は MIT の条文 (Copyright (c) 2025 Ionic)。
const LICENSE_OVERRIDES: Record<string, { version: string; license: string }> = {
	'@capacitor/synapse': { version: '1.0.4', license: 'MIT' }
};

export function thirdPartyLicenses(): Plugin {
	return {
		name: 'bp-carnet:third-party-licenses',
		apply: 'build',
		// サーバー側のビルドは配らないので、ブラウザに届く分だけを数える。
		applyToEnvironment: (environment) => environment.name === 'client',
		generateBundle(_options, bundle) {
			const root = this.environment.config.root;
			// FIX: ここで止めると、@vite-pwa/sveltekit が closeBundle で出力の無いことに失敗し、その ENOENT だけが
			// 表に出て、止めた理由が隠れる。理由はログにも書いておく。
			// 型の注釈を付けた宣言にしないと、呼んだ後で TypeScript が型を絞らない。
			const fail: (message: string) => never = (message) => {
				this.environment.logger.error(message);
				return this.error(message);
			};
			const packageDirs = new Set<string>();
			for (const output of Object.values(bundle)) {
				// JS は chunk に入ったモジュール、フォントなどのファイルは元の場所から辿る。
				const sources =
					output.type === 'chunk'
						? output.moduleIds
						: output.originalFileNames.map((name) => resolve(root, name));
				for (const source of sources) {
					if (source.startsWith('\0')) {
						const packageOf = VIRTUAL_MODULES[source];
						if (packageOf === undefined) {
							fail(
								`仮想モジュール ${source.slice(1)} のパッケージが分からない (VIRTUAL_MODULES に足す)`
							);
						}
						packageDirs.add(packageOf(root));
						continue;
					}
					const dir = packageDir(source);
					if (dir) packageDirs.add(dir);
				}
			}
			for (const name of EXTRA_PACKAGES) packageDirs.add(resolvePackage(root, name));
			const workboxBuild = ['@vite-pwa/sveltekit', 'vite-plugin-pwa', 'workbox-build'].reduce(
				resolvePackage,
				root
			);
			for (const name of WORKBOX_PACKAGES) packageDirs.add(resolvePackage(workboxBuild, name));

			const extracted: ExtractedPackage[] = [];
			for (const dir of packageDirs) {
				const manifest = JSON.parse(readFileSync(join(dir, 'package.json'), 'utf8'));
				const ownText = cutAtEnd(licenseText(dir), TEXT_END[manifest.name]);
				// 追加の本文を足す前に見る。足した後だと、本来の条文が欠けても空にならず気づけない。
				if (ownText.trim() === '') {
					fail(`${manifest.name} がライセンスの本文を同梱していない (${dir})`);
				}
				const text = ownText + extraTexts(dir, EXTRA_TEXT_FILES[manifest.name] ?? []);
				const override = LICENSE_OVERRIDES[manifest.name];
				if (override && override.version !== manifest.version) {
					fail(
						`${manifest.name} の版が ${manifest.version} に変わった。宣言と本文を確かめて LICENSE_OVERRIDES を直す`
					);
				}
				// 欄を持たないパッケージは本文から見分ける。
				const id =
					override?.license ?? licenseId(manifest) ?? (/^\s*MIT License/.test(text) ? 'MIT' : null);
				if (id === null) {
					fail(`${manifest.name} のライセンスが分からない (${dir})`);
				}
				if (!isAccepted(id)) {
					fail(
						`${manifest.name} のライセンス (${id}) を確かめて license-display.ts の ACCEPTED に足す (${dir})`
					);
				}
				extracted.push({
					name: manifest.name,
					version: manifest.version,
					license: id,
					repository: REPOSITORIES[manifest.name] ?? repositoryUrl(manifest.repository),
					texts: [{ id, name: id, text }]
				});
			}

			// pnpm は同じ版を peer ごとに別の場所へ置くので、同じ `名前@版` は1つにする。
			const unique = new Map(extracted.map((pkg) => [`${pkg.name}@${pkg.version}`, pkg]));
			let display;
			try {
				display = buildLicenseDisplay([...unique.values()]);
			} catch (error) {
				fail(error instanceof Error ? error.message : String(error));
			}

			this.emitFile({
				type: 'asset',
				fileName: NPM_LICENSES_FILE,
				source: JSON.stringify(display)
			});
		}
	};
}

/** モジュールの ID から、それを含むパッケージのディレクトリを返す。npm のパッケージでなければ `null`。 */
function packageDir(moduleId: string): string | null {
	const path = moduleId.replace(/[?#].*$/, '').replaceAll('\\', '/');
	const index = path.lastIndexOf(NODE_MODULES);
	if (index === -1) return null;
	const rest = path.slice(index + NODE_MODULES.length).split('/');
	const segments = rest[0].startsWith('@') ? 2 : 1;
	const dir = path.slice(0, index + NODE_MODULES.length) + rest.slice(0, segments).join('/');
	return existsSync(join(dir, 'package.json')) ? dir : null;
}

/**
 * `from` のパッケージから見た `name` のパッケージの場所。pnpm でも、npm が入れ子に置いても見つかる。
 * `require.resolve('<name>/package.json')` は、`exports` で package.json を出していないパッケージ
 * (@inlang/paraglide-js など) で失敗するので、Node が探す場所を順に見る。
 */
function resolvePackage(from: string, name: string): string {
	const paths = createRequire(join(from, 'package.json')).resolve.paths(name) ?? [];
	const dir = paths
		.map((base) => join(base, name))
		.find((d) => existsSync(join(d, 'package.json')));
	if (dir === undefined) throw new Error(`${name} が見つからない (${from} から探した)`);
	return realpathSync(dir);
}

/** パッケージのライセンス本文。持っていなければ空文字。 */
function licenseText(dir: string): string {
	const files = readdirSync(dir, { withFileTypes: true })
		// LICENSE がシンボリックリンクのパッケージがあるので、`isFile()` だけで絞らない。
		.filter((entry) => !entry.isDirectory() && isLicenseFileName(entry.name))
		.map((entry) => entry.name)
		.sort();
	return joinLicenseFiles(
		files.map((name) => ({ name, text: readFileSync(join(dir, name), 'utf8') }))
	);
}

/** EXTRA_TEXT_FILES のファイルを、どの条文かが分かるようファイル名を挟んで並べる。無ければビルドを止める。 */
function extraTexts(dir: string, names: string[]): string {
	return names
		.map((name) => {
			const path = join(dir, name);
			if (!existsSync(path)) throw new Error(`${path} が無い (EXTRA_TEXT_FILES を見直す)`);
			return `\n\n${name}\n\n${readFileSync(path, 'utf8')}`;
		})
		.join('');
}

/** `end` があれば、そこから後ろを落とす。印が見つからなければ、形が変わったのでビルドを止める。 */
function cutAtEnd(text: string, end: string | undefined): string {
	if (end === undefined) return text;
	const index = text.indexOf(end);
	if (index === -1) throw new Error(`本文に ${JSON.stringify(end)} が無い (TEXT_END を見直す)`);
	return text.slice(0, index).trimEnd() + '\n';
}

/** Vite のパッケージの実際の場所。pnpm ではシンボリックリンクの先が本物で、依存はその隣に並ぶ。 */
function viteDir(root: string): string {
	return realpathSync(resolve(root, 'node_modules', 'vite'));
}

/** `package.json` が書いているライセンス。旧い形式 (`{ type }`・`licenses`) も拾う。 */
function licenseId(manifest: Record<string, unknown>): string | null {
	const license = manifest.license ?? (manifest.licenses as unknown[] | undefined)?.[0];
	if (typeof license === 'string') return license;
	if (typeof license === 'object' && license !== null && 'type' in license) {
		const type = (license as { type: unknown }).type;
		if (typeof type === 'string') return type;
	}
	return null;
}
