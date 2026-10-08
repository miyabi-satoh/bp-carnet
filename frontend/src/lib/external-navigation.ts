/**
 * 外部URL (Stripe Checkout 等) へのフルナビゲーション。`window.location.assign` を直接呼ぶと、
 * 実ブラウザでは `location`・`assign` が再定義できずテストで差し替えられないため、
 * `isInAppBrowser`・`isIosWebApp` と同じく専用モジュールに切り出してモックする。
 */
export function navigateTo(url: string): void {
	window.location.assign(url);
}
