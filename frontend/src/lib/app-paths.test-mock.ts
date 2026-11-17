// 単体テストで `$app/paths` に替えるもの (`vi.mock('$app/paths', () => import('#lib/app-paths.test-mock.js'))`)。
// テストは SvelteKit の外で動き、本物の `resolve` は基点のパスを読めないため。

/** 基点を `/` として、`[param]` を `params` の値に置き換える。 */
export function resolve(path: string, params: Record<string, string> = {}): string {
	return `/${path.replace(/^\//, '')}`.replace(/\[(\w+)\]/g, (_, key: string) => params[key]);
}

export function asset(path: string): string {
	return path;
}
