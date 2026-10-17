import { fetchAnalyticsToken } from '$lib/auth';
import { isNativeApp } from '$lib/native-app';

/** Cloudflare Web Analytics のスクリプト (Cloudflare の資料の手で入れる形)。 */
const BEACON_SRC = 'https://static.cloudflareinsights.com/beacon.min.js';

let started = false;

/** ログインしていない画面に来たときに呼ぶ。ページを開いてから1回だけスクリプトを読み、そのときの画面だけを数える
 * (docs/architecture.md「アクセス解析」)。アプリでは読まない。 */
export async function startWebAnalytics(): Promise<void> {
	if (isNativeApp || started) return;
	started = true;
	const token = await fetchAnalyticsToken();
	if (!token) return;
	const script = document.createElement('script');
	script.type = 'module';
	script.src = BEACON_SRC;
	// 画面の移動は数えない。数えると、ログインした後の画面も計測に入るため。
	script.dataset.cfBeacon = JSON.stringify({ token, spa: false });
	document.body.append(script);
}
