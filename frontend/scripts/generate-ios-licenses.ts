// iPhone アプリに入るネイティブの部品のライセンス表示を作る (`just licenses`。→ docs/third-party-licenses.md)。
// 画面の分 (npm) はビルドのたびに Vite のプラグインが作るが、ネイティブの部品は npm のビルドからは見えないので、
// Swift のパッケージの一覧から集めて `static/third-party-licenses/ios.json` に書き出し、リポジトリに入れる。
//
// - Swift Package Manager で取る部品: `Package.resolved` の固定 (版とコミット) ごとに、上流のリポジトリから
//   そのコミットのライセンスのファイルを取る。取ったリポジトリは `data/license-cache/` に置き、次からは使い回す。
// - Capacitor のプラグイン: `CapApp-SPM/Package.swift` が node_modules の中を指しているもの。
//   このリポジトリの `mobile/plugins/` はこのアプリのコードなので数えない。
//
// `--check` を付けると取りに行かず、書き出してある一覧の部品と版が、上の2つの今の版と合っているかだけを見る
// (`just licenses-check`)。ネットを見ずに、Windows でも通せるようにするため。

import { execFileSync } from 'node:child_process';
import { existsSync, mkdirSync, readdirSync, readFileSync, writeFileSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import {
	buildLicenseDisplay,
	isAccepted,
	isLicenseFileName,
	joinLicenseFiles,
	repositoryUrl,
	unique,
	type ExtractedPackage,
	type LicenseDisplay
} from './license-display.ts';
import { dataDir, REPO_ROOT } from './repo-paths.ts';

const RESOLVED = join(
	REPO_ROOT,
	'mobile/ios/App/App.xcodeproj/project.xcworkspace/xcshareddata/swiftpm/Package.resolved'
);
const CAP_APP_SPM = join(REPO_ROOT, 'mobile/ios/App/CapApp-SPM');
const OUTPUT = join(REPO_ROOT, 'frontend/static/third-party-licenses/ios.json');
const CACHE = join(dataDir(), 'license-cache');

// 本文から見分けるライセンス。Swift のパッケージには npm の `license` の欄に当たるものが無いため。
// 見分けられない本文が来たら止め、条文を確かめてからここに足す。
const LICENSE_PATTERNS: [string, RegExp][] = [
	['Apache-2.0', /Apache License\s+Version 2\.0/],
	['MIT', /Permission is hereby granted, free of charge/],
	['BSD-3-Clause', /Redistribution and use in source and binary forms[\s\S]*Neither the name/]
];

interface Pin {
	identity: string;
	kind: string;
	location: string;
	state: { revision: string; version?: string };
}

interface Source {
	name: string;
	version: string;
	/** 本文を取り出す。`--check` では呼ばない。 */
	extract: () => ExtractedPackage;
}

function swiftPackages(): Source[] {
	const { pins } = JSON.parse(readFileSync(RESOLVED, 'utf8')) as { pins: Pin[] };
	return pins.map((pin) => {
		if (pin.kind !== 'remoteSourceControl' || !pin.state.version) {
			throw new Error(`${pin.identity}: 版で固定した上流のリポジトリではない (${pin.kind})`);
		}
		const repository = pin.location.replace(/\.git$/, '');
		const name = repository.split('/').at(-1)!;
		const version = pin.state.version;
		return {
			name,
			version,
			extract: () => {
				const files = gitLicenseFiles(pin);
				if (files.length === 0) throw new Error(`${name}@${version}: ライセンスのファイルが無い`);
				const license = unique(
					files.map(({ name: file, text }) => detect(`${name} ${file}`, text))
				);
				const text = joinLicenseFiles(files);
				const id = license.join(' AND ');
				return { name, version, license: id, repository, texts: [{ id, name: id, text }] };
			}
		};
	});
}

function capacitorPlugins(): Source[] {
	const manifest = readFileSync(join(CAP_APP_SPM, 'Package.swift'), 'utf8');
	return (
		[...manifest.matchAll(/\.package\(name: "[^"]+", path: "([^"]+)"\)/g)]
			// Package.swift に書いてある `/` 区切りの形で見る。解決した後のパスは Windows では `\` 区切りになる。
			.filter((match) => match[1].includes('/node_modules/'))
			.map((match) => resolve(CAP_APP_SPM, match[1]))
			.map((dir) => {
				const pkg = JSON.parse(readFileSync(join(dir, 'package.json'), 'utf8'));
				return {
					name: pkg.name,
					version: pkg.version,
					extract: () => {
						const files = readdirSync(dir)
							.filter(isLicenseFileName)
							.sort()
							.map((file) => ({ name: file, text: readFileSync(join(dir, file), 'utf8') }));
						if (files.length === 0)
							throw new Error(`${pkg.name}: ライセンスのファイルが無い (${dir})`);
						return {
							name: pkg.name,
							version: pkg.version,
							license: pkg.license,
							repository: repositoryUrl(pkg.repository),
							texts: [{ id: pkg.license, name: pkg.license, text: joinLicenseFiles(files) }]
						};
					}
				};
			})
	);
}

/** 固定したコミットの、リポジトリの直下にあるライセンスのファイル。 */
function gitLicenseFiles(pin: Pin): { name: string; text: string }[] {
	const dir = join(CACHE, `${pin.identity}.git`);
	if (!existsSync(dir)) {
		mkdirSync(dirname(dir), { recursive: true });
		git(CACHE, 'clone', '--bare', '--filter=blob:none', '--quiet', pin.location, dir);
	}
	try {
		git(dir, 'cat-file', '-e', `${pin.state.revision}^{commit}`);
	} catch {
		git(dir, 'fetch', '--quiet', '--filter=blob:none', 'origin', pin.state.revision);
	}
	return git(dir, 'ls-tree', '--name-only', pin.state.revision)
		.split('\n')
		.filter(isLicenseFileName)
		.sort()
		.map((file) => ({ name: file, text: git(dir, 'show', `${pin.state.revision}:${file}`) }));
}

function git(cwd: string, ...args: string[]): string {
	return execFileSync('git', args, { cwd, encoding: 'utf8', stdio: ['ignore', 'pipe', 'pipe'] });
}

/** 本文に入っている条文の式。取り込んだ部分の条文を後ろに続けるファイル (GoogleUtilities) があるので、全部を `AND` で並べる。 */
function detect(where: string, text: string): string {
	const found = LICENSE_PATTERNS.filter(([, pattern]) => pattern.test(text)).map(([id]) => id);
	if (found.length === 0) {
		throw new Error(`${where}: ライセンスを見分けられない。条文を確かめて LICENSE_PATTERNS を直す`);
	}
	return found.join(' AND ');
}

const sources = [...swiftPackages(), ...capacitorPlugins()];

if (process.argv.includes('--check')) {
	const want = sources.map((s) => `${s.name}@${s.version}`).sort();
	const written = JSON.parse(readFileSync(OUTPUT, 'utf8')) as LicenseDisplay;
	const have = written.packages.flatMap((p) => p.versions.map((v) => `${p.name}@${v}`)).sort();
	if (JSON.stringify(want) !== JSON.stringify(have)) {
		console.error(
			`iPhone アプリの部品の一覧が古い。\`just licenses\` で作り直してコミットする。\n今の部品: ${want.join(', ')}\n一覧: ${have.join(', ')}`
		);
		process.exit(1);
	}
} else {
	const extracted = sources.map((s) => s.extract());
	const rejected = extracted.filter((p) => !isAccepted(p.license));
	if (rejected.length > 0) {
		throw new Error(
			`配ってよいか確かめていないライセンスがある。確かめて license-display.ts の ACCEPTED に足す:\n${rejected.map((p) => `${p.name}@${p.version}: ${p.license}`).join('\n')}`
		);
	}
	mkdirSync(dirname(OUTPUT), { recursive: true });
	writeFileSync(OUTPUT, JSON.stringify(buildLicenseDisplay(extracted)) + '\n');
}
