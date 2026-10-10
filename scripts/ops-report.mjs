// Fly.io 上の本番の稼働・利用状況を、1コマンドで表示する (`just report`)。
//
// 1. Fly の状態 (読み取りのみ、Machine を起こさない): 起動・一時停止の回数、最後の終了コード。
// 2. アプリの状態 (Machine を起こす。`--no-wake` で省く): 起動を待つ時間、ユーザー数、
//    最終アクセスごとの人数、OCR の使用額、削除予約。管理者でログインして
//    `GET /api/v1/admin/users` を1回叩くだけで、血圧の記録そのものは読まない。
//
// 管理者の ID・パスワードは環境変数で渡す (引数に書くと `ps` に出るため)。
//   BP_REPORT_USER / BP_REPORT_PASSWORD
// 日ごとの推移 (新規登録・記録件数など) は、管理 API が返さないため出せない。

import { execFileSync } from 'node:child_process';

const args = process.argv.slice(2);
const option = (name, fallback) => {
	const i = args.indexOf(name);
	return i >= 0 && args[i + 1] ? args[i + 1] : fallback;
};
const baseUrl = option('--url', 'https://bp.amiiby.com').replace(/\/$/, '');
const app = option('--app', 'bp-carnet');
const noWake = args.includes('--no-wake');

const DAY_MS = 24 * 60 * 60 * 1000;
const now = Date.now();

function heading(text) {
	console.log(`\n■ ${text}`);
}

function flySection() {
	heading(`Fly.io (${app})`);
	let machines;
	try {
		machines = JSON.parse(
			execFileSync('fly', ['machines', 'list', '--app', app, '--json'], {
				encoding: 'utf8'
			})
		);
	} catch (error) {
		console.log(`  取得できませんでした: ${error.message.split('\n')[0]}`);
		return;
	}
	for (const machine of machines) {
		const events = machine.events ?? [];
		// 1回の起動で `starting`・`started` の両方が出ることがあるので、完了したものだけを数える。
		const within = (type, days, status) =>
			events.filter(
				(e) =>
					e.type === type &&
					(status === undefined || e.status === status) &&
					now - e.timestamp <= days * DAY_MS
			).length;
		const lastExit = events.find((e) => e.type === 'exit');
		console.log(`  Machine ${machine.id}: ${machine.state} (${machine.region})`);
		console.log(
			// 一時停止からの再開も `start` で出る。
			`  起動・再開 24時間 ${within('start', 1, 'started')} 回 / 7日 ${within('start', 7, 'started')} 回、` +
				`一時停止 24時間 ${within('suspension', 1, 'suspended')} 回 / 7日 ${within('suspension', 7, 'suspended')} 回、` +
				`停止 24時間 ${within('stop', 1)} 回 / 7日 ${within('stop', 7)} 回` +
				` (Fly が保持する直近 ${events.length} 件の範囲)`
		);
		if (lastExit) {
			// exit_code・oom_killed は、異常終了のときだけ付くことがある (正常停止では requested_stop だけ)。
			const exit = lastExit.request?.exit_event ?? {};
			const detail = [
				exit.requested_stop ? '要求による停止' : '要求なしの終了',
				exit.exit_code !== undefined ? `exit_code=${exit.exit_code}` : null,
				exit.oom_killed ? 'oom_killed=true' : null
			]
				.filter(Boolean)
				.join(' ');
			console.log(`  最後の終了: ${new Date(lastExit.timestamp).toISOString()} ${detail}`);
		}
	}
}

async function appSection() {
	heading(`アプリ (${baseUrl})`);
	const started = Date.now();
	const health = await fetch(`${baseUrl}/api/v1/health`).catch(() => null);
	const seconds = ((Date.now() - started) / 1000).toFixed(1);
	if (!health?.ok) {
		console.log(`  health: 失敗 (${health?.status ?? '接続できない'})`);
		return;
	}
	console.log(`  health: ok (${seconds} 秒。起きていなかったなら、起動・再開を待った時間を含む)`);

	const user = process.env.BP_REPORT_USER;
	const password = process.env.BP_REPORT_PASSWORD;
	if (!user || !password) {
		console.log('  BP_REPORT_USER / BP_REPORT_PASSWORD が未設定のため、利用状況は省きます。');
		return;
	}
	// 同一オリジンの検査 (src/csrf.rs) を通すため、Origin を付ける。
	const login = await fetch(`${baseUrl}/api/v1/auth/login`, {
		method: 'POST',
		headers: { 'content-type': 'application/json', origin: baseUrl },
		body: JSON.stringify({ username: user, password })
	});
	if (!login.ok) {
		console.log(`  管理者のログインに失敗しました (${login.status})`);
		return;
	}
	const cookie = login.headers
		.getSetCookie()
		.map((c) => c.split(';')[0])
		.join('; ');
	const res = await fetch(`${baseUrl}/api/v1/admin/users`, {
		headers: { cookie }
	});
	if (!res.ok) {
		console.log(`  ユーザー一覧を取得できませんでした (${res.status}。管理者ではない?)`);
		return;
	}
	const { users, ocrDefaultBudgetYen } = await res.json();

	const idleDays = (u) => (now - Date.parse(u.lastSeenAt)) / DAY_MS;
	const active = (days) => users.filter((u) => idleDays(u) <= days).length;
	const budgetOf = (u) => u.ocrBudgetYen ?? ocrDefaultBudgetYen;

	console.log(`  確認済みのユーザー: ${users.length} 人`);
	console.log(
		`    管理者 ${users.filter((u) => u.role === 'admin').length}、` +
			`凍結 ${users.filter((u) => u.frozen).length}`
	);
	console.log(
		`  最終アクセス: 24時間以内 ${active(1)} 人 / 7日以内 ${active(7)} 人 / 30日以内 ${active(30)} 人`
	);

	const spent = users.reduce((sum, u) => sum + u.ocrSpentYen, 0);
	const usersWithSpent = users.filter((u) => u.ocrSpentYen > 0).length;
	const exhausted = users.filter((u) => budgetOf(u) >= 0 && u.ocrSpentYen >= budgetOf(u)).length;
	console.log(
		`  OCR の使用額: 合計 ${spent} 円 (使った人 ${usersWithSpent} 人、予算を使い切った人 ${exhausted} 人)`
	);

	const scheduled = users.filter((u) => u.deletionScheduledAt);
	const byOrigin = (origin) => scheduled.filter((u) => u.deletionOrigin === origin).length;
	console.log(
		`  削除予約: ${scheduled.length} 人 (本人 ${byOrigin('user')}、管理者 ${byOrigin('admin')}、` +
			`放置 ${byOrigin('inactive')})`
	);
}

console.log(`稼働・利用状況 ${new Date(now).toISOString()}`);
flySection();
if (noWake) {
	console.log('\n(--no-wake: アプリには問い合わせていません)');
} else {
	await appSection();
}
