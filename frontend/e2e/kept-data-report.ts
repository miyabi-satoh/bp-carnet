// `E2E_KEEP_DATA=1` の実行の終わりに、残したユーザーと記録を一覧で出す (Playwright の globalTeardown、→ kept-data.ts)。
// テストが落ちても出す。
import { existsSync, readFileSync } from 'node:fs';
import { KEPT_DATA_FILE, type KeptItem } from './kept-data';

export default function reportKeptData(): void {
	const items: KeptItem[] = existsSync(KEPT_DATA_FILE)
		? readFileSync(KEPT_DATA_FILE, 'utf8')
				.split('\n')
				.filter((line) => line !== '')
				.map((line) => JSON.parse(line) as KeptItem)
		: [];
	const lines = [
		'',
		`E2E_KEEP_DATA: テストが作ったものを片付けずに残した (${process.env.E2E_BASE_URL})`,
		`控え: ${KEPT_DATA_FILE}`
	];
	if (items.length === 0) {
		lines.push('残したものは無い');
	}
	const users = items.filter((item) => item.kind === 'user');
	if (users.length > 0) {
		lines.push('', 'ユーザー (ID / パスワード / テスト):');
		for (const { username, password, test } of users) {
			lines.push(`  ${username}  ${password}  ${test}`);
		}
	}
	const records = items.filter((item) => item.kind === 'record');
	if (records.length > 0) {
		lines.push('', '記録 (持ち主 / メモ / テスト):');
		for (const { owner, memo, test } of records) {
			lines.push(`  ${owner}  ${memo}  ${test}`);
		}
	}
	if (items.length > 0) {
		lines.push('', '残したユーザーは管理画面から削除する (削除の予約になる)。');
	}
	console.log(lines.join('\n'));
}
