// iPhone アプリの Swift のパッケージが、入れてある Capacitor のプラグインと食い違っていないかを見る (`just ios-packages-check`)。
// プラグインを上げると、`cap update ios` が書く `CapApp-SPM/Package.swift` と、Xcode が解決して書く
// `Package.resolved` の両方が追従を要するが、どちらも手元で Xcode を通さないと変わらず、入れ漏れに気づけないため。
// - `Package.swift`: 先に `cap update ios` を流しておき (justfile)、ここでは書き換わっていないかを git で見る。
// - `Package.resolved`: 固定した版が、プラグインなどの `Package.swift` が求める範囲に入っているかを見る。
//   Xcode を使わずに Linux の CI でも見るため、解決はせず、書いてある範囲と照らすだけにする。

import { execFileSync } from 'node:child_process';
import { readFileSync } from 'node:fs';
import { join, resolve } from 'node:path';
import { REPO_ROOT } from './repo-paths.ts';

const IOS_DIR = 'mobile/ios';
const CAP_APP_SPM = join(REPO_ROOT, IOS_DIR, 'App/CapApp-SPM');
const RESOLVED = join(
	REPO_ROOT,
	IOS_DIR,
	'App/App.xcodeproj/project.xcworkspace/xcshareddata/swiftpm/Package.resolved'
);

const RESOLVE_HINT =
	'mac で xcodebuild -resolvePackageDependencies -project mobile/ios/App/App.xcodeproj -scheme App を流し、Package.resolved をコミットする';

// プレリリース (`-beta.1` など) は比べ方が変わるので読まない。今の依存には出てこない。
const PLAIN_VERSION = /^\d+(\.\d+){0,2}$/;

interface Pin {
	location: string;
	state: { version?: string };
}

const problems: string[] = [];

// `cap update ios` は、自前のプラグインの Package.swift の capacitor-swift-pm の版も Capacitor に合わせて書き換える。
const changed = execFileSync(
	'git',
	['status', '--porcelain', '--', CAP_APP_SPM, 'mobile/plugins/*/Package.swift'],
	{ cwd: REPO_ROOT, encoding: 'utf8' }
).trim();
if (changed !== '') {
	problems.push(
		`cap update ios で書き換わった (手元で just ios-packages-check を流し、書き換わったものをコミットする):\n${changed}`
	);
}

const pins = new Map(
	(JSON.parse(readFileSync(RESOLVED, 'utf8')) as { pins: Pin[] }).pins.map((pin) => [
		normalizeUrl(pin.location),
		pin.state.version
	])
);

const capAppManifest = readFileSync(join(CAP_APP_SPM, 'Package.swift'), 'utf8');
const manifests = [
	CAP_APP_SPM,
	...[...capAppManifest.matchAll(/\.package\(name:\s*"[^"]+",\s*path:\s*"([^"]+)"/g)].map((m) =>
		resolve(CAP_APP_SPM, m[1])
	)
];
for (const dir of manifests) {
	const manifest = readFileSync(join(dir, 'Package.swift'), 'utf8');
	const file = `${dir.slice(REPO_ROOT.length + 1)}/Package.swift`;
	const dependencies = [...manifest.matchAll(/\.package\(url:\s*"([^"]+)",\s*(.+?)\)\s*,?\s*$/gm)];
	if (dependencies.length !== (manifest.match(/\.package\(url:/g) ?? []).length) {
		problems.push(`${file}: .package(url: ...) の書き方を読めないものがある (この検査に足す)`);
	}
	for (const [, url, requirement] of dependencies) {
		const key = normalizeUrl(url);
		const pinned = pins.get(key);
		const where = `${file} の ${url}`;
		if (!pins.has(key)) {
			problems.push(`${where}: Package.resolved に固定が無い (${RESOLVE_HINT})`);
			continue;
		}
		const range = parseRequirement(requirement);
		if (range === undefined) {
			problems.push(`${where}: 求める版の書き方 (${requirement}) を読めない (この検査に足す)`);
		} else if (pinned === undefined || !PLAIN_VERSION.test(pinned)) {
			problems.push(
				`${where}: Package.resolved の固定 (${pinned ?? '版でなくブランチかコミット'}) を読めない (この検査に足す)`
			);
		} else if (!(compare(pinned, range.min) >= 0 && compare(pinned, range.below) < 0)) {
			problems.push(
				`${where}: ${requirement} を求めるが、Package.resolved は ${pinned} (${RESOLVE_HINT})`
			);
		}
	}
}

if (problems.length > 0) {
	console.error(`iPhone アプリの Swift のパッケージを確かめる:\n${problems.join('\n')}`);
	process.exit(1);
}

function normalizeUrl(url: string): string {
	return url.toLowerCase().replace(/\.git$/, '');
}

/** SwiftPM の書き方を、`min` 以上 `below` 未満の範囲にする。このリポジトリに出てくる書き方だけを読む。 */
function parseRequirement(text: string): { min: string; below: string } | undefined {
	const version = text.match(/"([^"]+)"/)?.[1];
	if (version === undefined || !PLAIN_VERSION.test(version)) return undefined;
	const exact = text.match(/^exact:\s*"([^"]+)"$/);
	if (exact) return { min: exact[1], below: bump(exact[1], 2) };
	const major = text.match(/^(?:from:\s*"([^"]+)"|\.upToNextMajor\(from:\s*"([^"]+)"\))$/);
	if (major) return { min: major[1] ?? major[2], below: bump(major[1] ?? major[2], 0) };
	const minor = text.match(/^\.upToNextMinor\(from:\s*"([^"]+)"\)$/);
	if (minor) return { min: minor[1], below: bump(minor[1], 1) };
	return undefined;
}

/** `index` の桁を1つ上げ、それより下の桁を 0 にする。 */
function bump(version: string, index: number): string {
	const parts = parse(version);
	return parts.map((n, i) => (i < index ? n : i === index ? n + 1 : 0)).join('.');
}

function compare(a: string, b: string): number {
	const [x, y] = [parse(a), parse(b)];
	return x[0] - y[0] || x[1] - y[1] || x[2] - y[2];
}

function parse(version: string): number[] {
	const parts = version.split('.').map(Number);
	return [parts[0] ?? 0, parts[1] ?? 0, parts[2] ?? 0];
}
