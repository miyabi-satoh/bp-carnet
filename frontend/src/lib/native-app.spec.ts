import { beforeEach, describe, expect, it, vi } from 'vitest';

// プラグインの保存の仕方をまねる: `set` は JSON にして保存し、`get` は戻し、`getItem` はそのまま返す。
const keychain = vi.hoisted(() => new Map<string, string>());
vi.mock('@aparajita/capacitor-secure-storage', () => ({
	KeychainAccess: { whenUnlockedThisDeviceOnly: 1 },
	SecureStorage: {
		set: async (key: string, data: unknown) => void keychain.set(key, JSON.stringify(data)),
		get: async (key: string) => {
			const data = keychain.get(key);
			return data === undefined ? null : JSON.parse(data);
		},
		getItem: async (key: string) => keychain.get(key) ?? null,
		remove: async (key: string) => keychain.delete(key)
	}
}));

const native = vi.hoisted(() => ({ value: false }));
vi.mock('@capacitor/core', () => ({ Capacitor: { isNativePlatform: () => native.value } }));
const printWebView = vi.hoisted(() => vi.fn());
vi.mock('@capgo/capacitor-printer', () => ({ Printer: { printWebView } }));

const writeFile = vi.hoisted(() => vi.fn());
const deleteFile = vi.hoisted(() => vi.fn());
vi.mock('@capacitor/filesystem', () => ({
	Directory: { Cache: 'CACHE' },
	Filesystem: { writeFile, deleteFile }
}));
const share = vi.hoisted(() => vi.fn());
vi.mock('@capacitor/share', () => ({ Share: { share } }));
const getInfo = vi.hoisted(() => vi.fn());
vi.mock('@capacitor/app', () => ({ App: { getInfo } }));

beforeEach(() => {
	native.value = false;
	writeFile.mockReset().mockResolvedValue({ uri: 'file:///cache/a.csv' });
	share.mockReset().mockResolvedValue(undefined);
	deleteFile.mockReset().mockResolvedValue(undefined);
	printWebView.mockReset();
	getInfo.mockReset().mockResolvedValue({ version: '1.2', build: '7' });
	keychain.clear();
	vi.resetModules();
});

describe('appToken', () => {
	it('保存したトークンを、起動し直しても同じ文字列で読める', async () => {
		await (await import('./native-app')).saveAppToken('tok');
		// 読み込み済みのトークンを持たない、起動し直した状態から読む。
		vi.resetModules();
		expect(await (await import('./native-app')).appToken()).toBe('tok');
	});

	it('消したら無い', async () => {
		const app = await import('./native-app');
		await app.saveAppToken('tok');
		await app.clearAppToken();
		vi.resetModules();
		expect(await (await import('./native-app')).appToken()).toBeNull();
	});
});

describe('appVersionLabel', () => {
	it('版にビルド番号を添える', async () => {
		expect(await (await import('./native-app')).appVersionLabel()).toBe('1.2 (7)');
	});

	it('読めなければ null で、次の呼び出しで読み直す', async () => {
		getInfo.mockRejectedValueOnce(new Error('unavailable'));
		const app = await import('./native-app');
		expect(await app.appVersionLabel()).toBeNull();
		expect(await app.appVersion()).toBe('1.2');
	});
});

describe('printPage', () => {
	it('アプリでは iOS の印刷を呼ぶ', async () => {
		native.value = true;
		await (await import('./native-app')).printPage();
		expect(printWebView).toHaveBeenCalled();
	});

	it('ウェブではブラウザの印刷を呼ぶ', async () => {
		const print = vi.fn();
		vi.stubGlobal('window', { print });
		await (await import('./native-app')).printPage();
		expect(print).toHaveBeenCalled();
		expect(printWebView).not.toHaveBeenCalled();
		vi.unstubAllGlobals();
	});
});

describe('saveFile (アプリ)', () => {
	beforeEach(() => {
		native.value = true;
	});

	it('中身をそのまま (BOM も) 書いて、共有シートに渡す', async () => {
		const blob = new Blob([new Uint8Array([0xef, 0xbb, 0xbf]), 'a,b\r\n']);
		await (await import('./native-app')).saveFile('a.csv', blob);
		const { data } = writeFile.mock.calls[0][0];
		expect(atob(data)).toBe('\xef\xbb\xbfa,b\r\n');
		expect(share).toHaveBeenCalledWith({ files: ['file:///cache/a.csv'] });
		expect(deleteFile).toHaveBeenCalledWith({ path: 'a.csv', directory: 'CACHE' });
	});

	it('共有に失敗しても、書いたファイルを消す', async () => {
		share.mockRejectedValue(new Error('boom'));
		deleteFile.mockRejectedValue(new Error('gone'));
		await expect((await import('./native-app')).saveFile('a.csv', new Blob(['x']))).rejects.toThrow(
			'boom'
		);
		expect(deleteFile).toHaveBeenCalled();
	});

	it('共有シートを閉じただけなら成功', async () => {
		share.mockRejectedValue(new Error('Share canceled'));
		await expect(
			(await import('./native-app')).saveFile('a.csv', new Blob(['x']))
		).resolves.toBeUndefined();
	});

	it('それ以外の失敗は伝える', async () => {
		share.mockRejectedValue(new Error('boom'));
		await expect((await import('./native-app')).saveFile('a.csv', new Blob(['x']))).rejects.toThrow(
			'boom'
		);
	});
});

describe('saveFile (ウェブ)', () => {
	it('ダウンロードさせ、共有シートは使わない', async () => {
		const anchor = { click: vi.fn(), remove: vi.fn(), href: '', download: '' };
		vi.stubGlobal('document', {
			createElement: () => anchor,
			body: { appendChild: vi.fn() }
		});
		vi.spyOn(URL, 'createObjectURL').mockReturnValue('blob:x');
		await (await import('./native-app')).saveFile('a.csv', new Blob(['x']));
		expect(anchor).toMatchObject({ href: 'blob:x', download: 'a.csv' });
		expect(anchor.click).toHaveBeenCalled();
		expect(writeFile).not.toHaveBeenCalled();
		vi.unstubAllGlobals();
		vi.restoreAllMocks();
	});
});
