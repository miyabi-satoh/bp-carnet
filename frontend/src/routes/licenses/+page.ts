import { asset } from '$app/paths';
import type { LicenseDisplay } from '../../../scripts/license-display';
import type { PageLoad } from './$types';

// 画面の分はビルドのたびに作り、iPhone アプリの分はリポジトリに入れてある (→ docs/third-party-licenses.md)。
// どちらも静的ファイルなので、API ではなくそのまま取る。iPhone アプリでは同梱した物を読むので、ネットが無くても出る。
const FILES = {
	web: asset('/third-party-licenses/npm.json'),
	ios: asset('/third-party-licenses/ios.json')
} as const;

export const load: PageLoad = async ({ fetch }) => {
	const [web, ios] = await Promise.all([
		fetchLicenses(fetch, FILES.web),
		fetchLicenses(fetch, FILES.ios)
	]);
	return { web, ios };
};

/** 一覧を取る。取れなければ `null`。開発サーバーにはビルドで作る分が無いので、片方が欠けてもページは出す。 */
async function fetchLicenses(
	fetch: typeof globalThis.fetch,
	path: string
): Promise<LicenseDisplay | null> {
	try {
		const response = await fetch(path);
		if (!response.ok) return null;
		return (await response.json()) as LicenseDisplay;
	} catch {
		return null;
	}
}
