// `E2E_KEEP_DATA=1` のとき、テストが作ったユーザーと記録を片付けずに残す。流した後に手でログインして、
// 画面を確かめたり操作の続きを試したりするため (起動済みの環境に当てる `just e2e` だけ)。
// 残したものは控えに書き、実行の終わりに一覧で出す (→ kept-data-report.ts)。消すときは管理画面から。
import { appendFileSync, mkdirSync } from 'node:fs';
import path from 'node:path';
import type { TestInfo } from '@playwright/test';

export const KEEP_DATA = process.env.E2E_KEEP_DATA === '1';

/** 残したものの控え。ワーカーごとのプロセスから追記するため、1件を1行の JSON にする。 */
export const KEPT_DATA_FILE = path.join(import.meta.dirname, '.kept', 'kept-data.jsonl');

export type KeptItem =
	| { kind: 'user'; test: string; username: string; password: string }
	| { kind: 'record'; test: string; owner: string; memo: string };

/** 残したものを控えに書く。呼び出し側は、実際に残っているものだけを渡す。 */
export function recordKept(item: KeptItem): void {
	mkdirSync(path.dirname(KEPT_DATA_FILE), { recursive: true });
	appendFileSync(KEPT_DATA_FILE, `${JSON.stringify(item)}\n`);
}

/** 一覧でどのテストが残したかを示す名前。先頭のファイル名は短くする。 */
export function keptTestName(testInfo: TestInfo): string {
	const [file, ...titles] = testInfo.titlePath;
	return [path.basename(file), ...titles].join(' › ');
}
