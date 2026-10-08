import { KeychainAccess, SecureStorage } from '@aparajita/capacitor-secure-storage';
import { Printer } from '@capgo/capacitor-printer';
import { App, type AppInfo } from '@capacitor/app';
import { Capacitor, SystemBars, SystemBarsStyle } from '@capacitor/core';
import { Directory, Filesystem } from '@capacitor/filesystem';
import { Share } from '@capacitor/share';

/** iPhone・iPad のアプリ (Capacitor) の中で動いているか (docs/mobile-app.md)。 */
export const isNativeApp = Capacitor.isNativePlatform();

/** API の呼び先のオリジン。アプリは画面を同梱していてページのオリジンが `capacitor://localhost` なので、
 * 公開 URL を呼ぶ。ウェブは同じオリジン。`VITE_APP_API_ORIGIN` は、シミュレータから開発用の
 * サーバーにつなぐときの差し替え。 */
export const API_ORIGIN = isNativeApp
	? import.meta.env.VITE_APP_API_ORIGIN || 'https://bp.amiiby.com'
	: '';

let cachedInfo: Promise<AppInfo | null> | undefined;

/** 読めなければ `null` で、次の呼び出しで読み直す。 */
function appInfo(): Promise<AppInfo | null> {
	cachedInfo ??= App.getInfo().catch(() => {
		cachedInfo = undefined;
		return null;
	});
	return cachedInfo;
}

/** アプリの版 (`CFBundleShortVersionString`)。サーバーが古い版を見分けられるよう、API の呼び出しに付ける
 * (docs/mobile-app.md)。読めなければ `null` で、次の呼び出しで読み直す (版を付けないと断られるため)。 */
export async function appVersion(): Promise<string | null> {
	return (await appInfo())?.version ?? null;
}

/** 設定に出す版。問い合わせで TestFlight のビルドまで見分けられるよう、ビルド番号 (`CFBundleVersion`) を添える。 */
export async function appVersionLabel(): Promise<string | null> {
	const info = await appInfo();
	return info ? `${info.version} (${info.build})` : null;
}

const TOKEN_KEY = 'app-token';

/** 読み込んだトークン。`undefined` はまだキーチェーンを読んでいない。 */
let cachedToken: string | null | undefined;

/** ログインの証しのトークン (docs/mobile-app.md)。無ければ `null`。 */
export async function appToken(): Promise<string | null> {
	if (cachedToken === undefined) {
		// `set` は JSON にして保存するので、読むのも `get` (`getItem` は JSON のまま返す)。
		const stored = await SecureStorage.get(TOKEN_KEY, false);
		cachedToken = typeof stored === 'string' ? stored : null;
	}
	return cachedToken;
}

/** ADR: 端末のロックを解いている間だけ読め、バックアップから別の端末へは移さない。
 * 移した先ではサーバーのトークンと結び付かない端末になるので、ログインし直してもらう。 */
export async function saveAppToken(token: string): Promise<void> {
	await SecureStorage.set(
		TOKEN_KEY,
		token,
		false,
		false,
		KeychainAccess.whenUnlockedThisDeviceOnly
	);
	cachedToken = token;
}

/** キーチェーンから消せなくても、この起動の間はトークンを送らない。 */
export async function clearAppToken(): Promise<void> {
	cachedToken = null;
	await SecureStorage.remove(TOKEN_KEY);
}

/** ステータスバーの文字色を、画面のライト・ダークに合わせる。既定は端末の設定に従うので、
 * 画面だけをダークにすると、暗い地に暗い文字が並んで読めなくなる (docs/mobile-app.md)。 */
export async function setStatusBarDark(dark: boolean): Promise<void> {
	if (!isNativeApp) return;
	await SystemBars.setStyle({ style: dark ? SystemBarsStyle.Dark : SystemBarsStyle.Light });
}

/** 印刷の画面を開く。アプリの WebView では `window.print()` が何もしないので、iOS の印刷を呼ぶ (docs/mobile-app.md)。 */
export async function printPage(): Promise<void> {
	if (isNativeApp) await Printer.printWebView({ name: 'BP Carnet' });
	else window.print();
}

/** ファイルを保存させる。アプリの WebView ではダウンロード (`a[download]`) が何も起こさないので、
 * 共有シート (「ファイルに保存」など) で渡す (docs/mobile-app.md)。 */
export async function saveFile(name: string, blob: Blob): Promise<void> {
	if (isNativeApp) await shareFile(name, blob);
	else downloadFile(name, blob);
}

function downloadFile(name: string, blob: Blob): void {
	const url = URL.createObjectURL(blob);
	const anchor = document.createElement('a');
	anchor.href = url;
	anchor.download = name;
	document.body.appendChild(anchor);
	anchor.click();
	anchor.remove();
	// Firefox/Safari はダウンロード自体が非同期に始まるため、click() 直後に同期的に
	// revoke すると (実装によっては) ダウンロードが失敗することがある。1秒後まで
	// 遅らせて安全側に倒す。
	setTimeout(() => URL.revokeObjectURL(url), 1000);
}

/** 共有シートを閉じただけなら成功として扱う。 */
async function shareFile(name: string, blob: Blob): Promise<void> {
	// 中身をそのまま書く (文字列にすると CSV の BOM が落ちる)。
	const { uri } = await Filesystem.writeFile({
		path: name,
		data: await toBase64(blob),
		directory: Directory.Cache
	});
	try {
		await Share.share({ files: [uri] });
	} catch (error) {
		// 共有シートを閉じると、プラグインは "Share canceled" で失敗を返す。
		if (!(error instanceof Error && error.message === 'Share canceled')) throw error;
	} finally {
		// 共有を終えたら (保存先へ写し終えている)、記録をキャッシュに残さない。消せなくても次の書き出しで上書きされる。
		await Filesystem.deleteFile({ path: name, directory: Directory.Cache }).catch(() => {});
	}
}

async function toBase64(blob: Blob): Promise<string> {
	const bytes = new Uint8Array(await blob.arrayBuffer());
	let binary = '';
	// 引数が多すぎると String.fromCharCode が失敗するので、区切って渡す。
	for (let i = 0; i < bytes.length; i += 0x8000) {
		binary += String.fromCharCode(...bytes.subarray(i, i + 0x8000));
	}
	return btoa(binary);
}
