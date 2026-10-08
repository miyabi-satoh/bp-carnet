/**
 * LINE・Instagram・Facebook などのアプリの中で開く簡易なブラウザ (内蔵ブラウザ) か。
 * Google はこの種のブラウザからのログインを拒否し、LINE も戻り先が別のブラウザになって失敗する
 * (docs/authentication.md)。User-Agent に頼る推測なので、誤って弾いても困らない範囲の目印だけを
 * 挙げる (LINE の `Line/` は実機で確かめること)。
 */
const IN_APP_BROWSER_PATTERN = /\bLine\/|Instagram|FBAN|FBAV/;

export function isInAppBrowser(): boolean {
	if (typeof navigator === 'undefined') return false;
	return IN_APP_BROWSER_PATTERN.test(navigator.userAgent ?? '');
}
