/**
 * iOS のホーム画面から全画面 (「Webアプリとして開く」) で起動しているか。
 * 実機 (iOS 26.6) では、この起動のときだけ `navigator.standalone` が true になる。
 * 標準の `display-mode: standalone` は、この起動でも false (browser のまま) なので使えない。
 * 「Webアプリとして開く」をオフにして追加したアイコンや、通常のブラウザでは false。
 */
export function isIosWebApp(): boolean {
	if (typeof navigator === 'undefined') return false;
	return (navigator as Navigator & { standalone?: boolean }).standalone === true;
}
