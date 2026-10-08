import { paraglideVitePlugin } from '@inlang/paraglide-js';
import { readFileSync } from 'node:fs';
import path from 'node:path';
import tailwindcss from '@tailwindcss/vite';
import { defineConfig } from 'vitest/config';
import { playwright } from '@vitest/browser-playwright';
import adapter from '@sveltejs/adapter-static';
import { sveltekit } from '@sveltejs/kit/vite';
import { mdsvex } from 'mdsvex';
import { SvelteKitPWA } from '@vite-pwa/sveltekit';
import { thirdPartyLicenses } from './scripts/third-party-licenses.ts';
import { dataDir } from './scripts/repo-paths.ts';

/**
 * just dev-backend の BP_CARNET_HOME (`data/dev-runtime`) にある config.toml から
 * `[server]` の `port` だけを拾う。ファイルが無い・値が無い・値が不正な場合は backend 側の
 * デフォルト (`src/config.rs`) と同じ 3000 を返す。TOML 全体をパースするほどの複雑さは
 * このファイルに無いため専用パッケージは増やさないが、TOML の整数リテラル (`3_000` の
 * ような桁区切り、`0x`/`0o`/`0b` 表記) は backend 側 (`toml` クレート) が受理する値と
 * ずれないよう最低限フォローする。
 */
function readBackendPort(): string {
	const configPath = path.join(dataDir(), 'dev-runtime', 'config.toml');
	let text: string;
	try {
		text = readFileSync(configPath, 'utf-8');
	} catch {
		return '3000';
	}

	let inServerSection = false;
	for (const line of text.split('\n')) {
		const trimmed = line.trim();
		if (trimmed.startsWith('[')) {
			inServerSection = trimmed === '[server]';
			continue;
		}
		if (!inServerSection) continue;
		const match = trimmed.match(/^port\s*=\s*([+-]?(?:0[xXoObB])?[0-9a-fA-F_]+)/);
		if (!match) continue;
		const port = Number(match[1].replace(/_/g, ''));
		// port = 0 は backend 側 (config.rs) の設定検証で既に拒否しているが、
		// 万一 config.toml に残っていた場合の保険として、ここでも無効値として扱う。
		return Number.isInteger(port) && port > 0 && port <= 65535 ? String(port) : '3000';
	}
	return '3000';
}

const backendPort = readBackendPort();

type HastNode = { tagName?: string; properties?: Record<string, unknown>; children?: HastNode[] };

/**
 * mdsvex は外部の URL へのリンクに必ず `rel="nofollow"` を付け、外す設定も無い (0.12)。
 * 本文のリンクは運営者のサイトや公的な資料なので、付けずに描く。
 */
function removeNofollow() {
	const walk = (node: HastNode) => {
		if (node.tagName === 'a') delete node.properties?.rel;
		node.children?.forEach(walk);
	};
	return walk;
}

export default defineConfig({
	plugins: [
		tailwindcss(),
		thirdPartyLicenses(),
		sveltekit({
			// 規約類 (`$lib/legal/<言語>/*.md`) と使い方の本文 (`$lib/help/<言語>/*.md`) を Svelte のコンポーネントにする。
			extensions: ['.svelte', '.md'],
			// 文中の引用符 ("" '') を曲がった引用符に変えない。
			preprocess: mdsvex({
				extensions: ['.md'],
				smartypants: false,
				rehypePlugins: [removeNofollow]
			}),
			compilerOptions: {
				// Force runes mode for the project, except for libraries. Can be removed in svelte 6.
				runes: ({ filename }) =>
					filename.split(/[/\\]/).includes('node_modules') ? undefined : true
			},
			// e2e (Playwright)・開発のスクリプトとアプリの設定も svelte-check の型検査と、型情報を使う eslint の対象にする。
			typescript: {
				config: (config) => {
					config.include.push(
						'../e2e/**/*.ts',
						'../scripts/**/*.ts',
						'../playwright.config.ts',
						'../capacitor.config.ts'
					);
				}
			},
			adapter: adapter({
				fallback: 'index.html'
			})
		}),

		// docs/architecture.md: Service Worker は静的アセットだけをキャッシュし、API の応答はキャッシュしない。
		// 登録はセキュアコンテキストのときだけ `+layout.svelte` で行うため、自動の登録は切る。
		SvelteKitPWA({
			injectRegister: false,
			registerType: 'autoUpdate',
			kit: { spa: true, adapterFallback: 'index.html' },
			manifest: {
				name: 'BP Carnet',
				short_name: 'BP Carnet',
				lang: 'ja',
				start_url: '/',
				scope: '/',
				display: 'standalone',
				// layout.css のライトの --background。
				background_color: '#fbf5ed',
				theme_color: '#fbf5ed',
				// `static/` の PNG は src/lib/assets/favicon.svg (ライトの配色) から rsvg-convert で作った。
				// maskable は、端末ごとの形に切り抜かれても欠けないよう地を正方形に塗った版。
				icons: [
					{ src: '/pwa-192x192.png', sizes: '192x192', type: 'image/png' },
					{ src: '/pwa-512x512.png', sizes: '512x512', type: 'image/png' },
					{
						src: '/maskable-icon-512x512.png',
						sizes: '512x512',
						type: 'image/png',
						purpose: 'maskable'
					}
				]
			},
			workbox: {
				// 新しい Service Worker をすぐ有効にし、開いているタブも読み直させる (autoUpdate)。
				// プラグインがこの2つを自動で入れるのは injectRegister が auto のときだけで、ここでは明示が要る。
				// 入れないと、古い画面が開いている間は古いフロントが配信され続け、新しい API と食い違う。
				skipWaiting: true,
				clientsClaim: true,
				// 先読みの対象は既定のまま (日本語フォントの woff2 は含まれない)。ビルド時の
				// `prerendered/**` が一致しないという警告は、事前生成するページの無い SPA では常に出る。
				// Google ログインのリダイレクトや CSV のダウンロードなど、`/api` への画面遷移は
				// index.html に差し替えずサーバーへ届ける。
				navigateFallbackDenylist: [/^\/api\//]
			}
		}),

		paraglideVitePlugin({
			project: './project.inlang',
			outdir: './src/lib/paraglide',
			emitTsDeclarations: true,
			// ADR: 公式の「ブラウザの表示言語で出し、選んだ言語で上書きする」形 (https://paraglidejs.com/strategy) から、
			// URL で言語を分けないので `url` を除いた。SPA でサーバーが言語を読まないので、`cookie` ではなく `localStorage` に残す。
			// Paraglide は最初に決まった言語を並べた保存先に書き込むので、2回目からは `localStorage` の言語になる。
			strategy: ['localStorage', 'preferredLanguage', 'baseLocale']
		})
	],
	server: {
		proxy: {
			// backend の同一オリジンチェック (CSRF対策, src/csrf.rs) はリクエストの Host ヘッダーを
			// 見る。文字列指定 (省略記法) だと Vite が changeOrigin: true を暗黙に付与し、backend が
			// 受け取る Host が target 側 (localhost:3000) に書き換わって Origin (localhost:5173) と
			// 不一致になり、ログイン等 GET 以外の全リクエストが 403 になってしまうため明示的に false にする。
			// ポートは data/dev-runtime/config.toml の [server].port (readBackendPort) に
			// 追従する。
			'/api': { target: `http://localhost:${backendPort}`, changeOrigin: false }
		}
	},
	test: {
		expect: { requireAssertions: true },
		projects: [
			{
				extends: './vite.config.ts',
				test: {
					name: 'client',
					browser: {
						enabled: true,
						provider: playwright(),
						instances: [{ browser: 'chromium', headless: true }]
					},
					include: ['src/**/*.svelte.{test,spec}.{js,ts}'],
					exclude: ['src/lib/server/**']
				}
			},

			{
				extends: './vite.config.ts',
				test: {
					name: 'server',
					environment: 'node',
					include: ['src/**/*.{test,spec}.{js,ts}'],
					exclude: ['src/**/*.svelte.{test,spec}.{js,ts}']
				}
			}
		]
	}
});
