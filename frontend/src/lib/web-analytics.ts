import { fetchAnalyticsToken, isPublicRoute } from '$lib/auth';
import { isNativeApp } from '$lib/native-app';

/** Cloudflare Web Analytics のスクリプト (Cloudflare の資料の手で入れる形)。 */
const BEACON_SRC = 'https://static.cloudflareinsights.com/beacon.min.js';

/** ログインしていなくても開けるが、数えない画面。メールのリンクの URL に使い捨ての鍵が付き、スクリプトは URL を
 * クエリ文字列ごと送るため。 */
const UNCOUNTED_ROUTES: readonly string[] = ['/change-email', '/reset-password', '/verify-email'];

/** アクセス解析で数える画面か (docs/architecture.md「アクセス解析」)。 */
export function isAnalyticsRoute(routeId: string | null): boolean {
	return isPublicRoute(routeId) && !UNCOUNTED_ROUTES.includes(routeId ?? '');
}

let started = false;

/** 数える画面に来たときに呼ぶ。ページを開いてから1回だけスクリプトを読み、そのときの画面だけを数える
 * (docs/architecture.md「アクセス解析」)。アプリでは読まない。`stillCounted` は、トークンが返った時点でも
 * 数える画面にいるか (問い合わせの間にログインして移っていたら読まない)。 */
export async function startWebAnalytics(stillCounted: () => boolean): Promise<void> {
	if (isNativeApp || started) return;
	started = true;
	const token = await fetchAnalyticsToken();
	if (!token || !stillCounted()) return;
	const script = document.createElement('script');
	script.type = 'module';
	script.src = BEACON_SRC;
	// 画面の移動は数えない。数えると、ログインした後の画面も計測に入るため。
	script.dataset.cfBeacon = JSON.stringify({ token, spa: false });
	document.body.append(script);
}
