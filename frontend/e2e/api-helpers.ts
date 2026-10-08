// e2e の準備と後片付けを API で行う。既存の環境にも流すため、テストが作ったものは必ず消す
// (`E2E_KEEP_DATA=1` で残すときを除く、→ kept-data.ts)。
// 確かめたいのは画面の動きなので、準備まで画面で組むと遅く、壊れる箇所も増える。
// `request` はブラウザのコンテキストのもの (`page.request`・`context.request`) か、管理者の
// `adminRequest` (→ fixtures.ts) を渡し、そのログイン状態で呼ぶ。
import type { APIRequestContext } from '@playwright/test';
import { addDays } from './date-helpers';

/** API を呼び、失敗したら例外にする。204 なら `null` を返す。 */
export async function send(
	request: APIRequestContext,
	method: 'get' | 'post' | 'put' | 'delete',
	apiPath: string,
	data?: unknown
): Promise<unknown> {
	const res = await request[method](`/api/v1${apiPath}`, { data });
	if (!res.ok()) {
		throw new Error(
			`${method.toUpperCase()} ${apiPath} が失敗しました (${res.status()}): ${await res.text()}`
		);
	}
	return res.status() === 204 ? null : res.json();
}

/**
 * 後片付けで API を呼ぶ。テスト本体の失敗を覆い隠さず、続く後片付けも止めないよう、
 * ここでの失敗 (例外を含む) は警告に留める。404 は既に無いものとして扱う。
 */
async function sendForCleanup(
	request: APIRequestContext,
	method: 'post' | 'delete',
	apiPath: string,
	what: string
): Promise<void> {
	try {
		const res = await request[method](`/api/v1${apiPath}`);
		if (!res.ok() && res.status() !== 404) {
			console.warn(`${what}の後片付けに失敗しました: ${res.status()}`);
		}
	} catch (err) {
		console.warn(`${what}の後片付けに失敗しました:`, err);
	}
}

/**
 * 設定を、朝・夜の時間帯は既定 (朝 4:00-10:00・夜 18:00-24:00)、タイムゾーンは手動の `timezone` にする。
 * 期待値をサーバーの既定値に頼らず決めるときに使う (自動のままだと、画面を開いたブラウザのタイムゾーンに揃う)。
 */
export async function resetSettingsByApi(
	request: APIRequestContext,
	timezone: string
): Promise<void> {
	await send(request, 'put', '/settings', {
		timezone,
		timezoneAuto: false,
		morningStartMin: 4 * 60,
		morningEndMin: 10 * 60,
		eveningStartMin: 18 * 60,
		eveningEndMin: 24 * 60
	});
}

/** ログイン中ユーザーのタイムゾーン。「今週」などの日付は、このタイムゾーンで求める。 */
export async function fetchTimeZone(request: APIRequestContext): Promise<string> {
	return ((await send(request, 'get', '/auth/me')) as { timezone: string }).timezone;
}

export type NewRecord = {
	/** ユーザーのタイムゾーンでの日時 (`YYYY-MM-DDTHH:MM`)。 */
	localMeasuredAt: string;
	systolic: number;
	diastolic: number;
	pulse?: number;
	memo?: string;
};

/** 記録を作り、id を返す。 */
export async function createRecordByApi(
	request: APIRequestContext,
	record: NewRecord
): Promise<number> {
	return ((await send(request, 'post', '/records', record)) as { id: number }).id;
}

/** 記録を `version` の版から直す。ほかの端末で直したことにするのに使う。 */
export async function updateRecordByApi(
	request: APIRequestContext,
	id: number,
	record: NewRecord,
	version: number
): Promise<void> {
	await send(request, 'put', `/records/${id}`, { ...record, version });
}

/** 記録を `version` の版で消す。ほかの端末で消したことにするのに使う。 */
export async function deleteRecordByApi(
	request: APIRequestContext,
	id: number,
	version: number
): Promise<void> {
	await send(request, 'delete', `/records/${id}?version=${version}`);
}

/** 記録を順に作る。 */
export async function createRecordsByApi(
	request: APIRequestContext,
	records: readonly NewRecord[]
): Promise<void> {
	for (const record of records) await createRecordByApi(request, record);
}

/**
 * `firstDay` から3日分の朝 (7:00)・夜 (21:00) の記録。グラフが4系列とも描ける。
 * 数値は見本と分かる値にし、最初の記録のメモは「起床後」にする。
 */
export function threeDaysOfMorningAndEvening(firstDay: string): NewRecord[] {
	const records = [0, 1, 2].flatMap((offset): NewRecord[] => [
		{
			localMeasuredAt: `${addDays(firstDay, offset)}T07:00`,
			systolic: 120 + offset * 2,
			diastolic: 80,
			pulse: 70
		},
		{
			localMeasuredAt: `${addDays(firstDay, offset)}T21:00`,
			systolic: 126 - offset * 2,
			diastolic: 84,
			pulse: 68
		}
	]);
	records[0].memo = '起床後';
	return records;
}

/** 後片付け専用。失敗は警告に留める (→ sendForCleanup)。 */
async function removeRecordByApi(
	request: APIRequestContext,
	{ id, version }: RecordForCleanup
): Promise<void> {
	await sendForCleanup(request, 'delete', `/records/${id}?version=${version}`, `記録 (id=${id}) `);
}

/**
 * ログイン中ユーザーの記録のうち、`memos` のいずれかをメモに持つものを消す。後片付け専用。
 * 失敗は警告に留める。
 */
export async function removeRecordsByMemoByApi(
	request: APIRequestContext,
	memos: ReadonlySet<string>
): Promise<void> {
	for (const record of await listRecordsForCleanup(request)) {
		if (memos.has(record.memo)) await removeRecordByApi(request, record);
	}
}

/**
 * ログイン中ユーザーの記録をすべて消す。後片付け専用で、使い捨てユーザーにだけ使う。
 * 失敗は警告に留める。
 */
export async function removeAllRecordsByApi(request: APIRequestContext): Promise<void> {
	for (const record of await listRecordsForCleanup(request))
		await removeRecordByApi(request, record);
}

/**
 * ログイン中ユーザーの記録のうち、`memos` のいずれかをメモに持つもののメモを返す。
 * 片付けずに残すとき (→ kept-data.ts) に、実際に残った記録だけを控えるために使う。
 */
export async function findRecordMemosByApi(
	request: APIRequestContext,
	memos: ReadonlySet<string>
): Promise<string[]> {
	return (await listRecordsForCleanup(request))
		.map(({ memo }) => memo)
		.filter((memo) => memos.has(memo));
}

type RecordForCleanup = { id: number; version: number; memo: string };

/** 後片付けや残したものの控えのために記録の一覧を取る。取れなければ警告して空を返す。 */
async function listRecordsForCleanup(request: APIRequestContext): Promise<RecordForCleanup[]> {
	try {
		return (await send(request, 'get', '/records')) as RecordForCleanup[];
	} catch (err) {
		console.warn('記録の一覧を取れませんでした:', err);
		return [];
	}
}

/** 一般ユーザーを作り、id を返す。呼べるのは管理者のログイン状態だけ。 */
export async function createUserByApi(
	request: APIRequestContext,
	username: string,
	password: string
): Promise<number> {
	const user = (await send(request, 'post', '/admin/users', {
		username,
		password,
		role: 'user'
	})) as { id: number };
	return user.id;
}

/**
 * ユーザーを消す。後片付け専用で、失敗は警告に留める。呼べるのは管理者のログイン状態だけ。
 * 管理者の API には即時の削除が無く、削除の予約になる (猶予期間を過ぎるとサーバーが消す、
 * docs/authentication.md)。
 */
export async function removeUserByApi(request: APIRequestContext, id: number): Promise<void> {
	await sendForCleanup(request, 'post', `/admin/users/${id}/deletion`, `ユーザー (id=${id}) `);
}

export type AdminUserSummary = {
	id: number;
	username: string;
	deletionScheduledAt?: string | null;
};

/** ユーザーの一覧。呼べるのは管理者のログイン状態だけ。 */
async function listUsersByApi(request: APIRequestContext): Promise<AdminUserSummary[]> {
	return ((await send(request, 'get', '/admin/users')) as { users: AdminUserSummary[] }).users;
}

/** ユーザー ID でユーザーを探す。呼べるのは管理者のログイン状態だけ。 */
export async function findUserByApi(
	request: APIRequestContext,
	username: string
): Promise<AdminUserSummary | undefined> {
	return (await listUsersByApi(request)).find((user) => user.username === username);
}

/**
 * `usernames` のいずれかをユーザー ID に持つユーザーを消す。後片付け専用で、失敗は警告に留める。
 * 呼べるのは管理者のログイン状態だけ。
 */
export async function removeUsersByUsernameByApi(
	request: APIRequestContext,
	usernames: ReadonlySet<string>
): Promise<void> {
	for (const { id, username } of await listUsersForCleanup(request)) {
		if (usernames.has(username)) await removeUserByApi(request, id);
	}
}

/**
 * `usernames` のうち、実在するユーザー ID を返す。呼べるのは管理者のログイン状態だけ。
 * 片付けずに残すとき (→ kept-data.ts) に、実際に作られたユーザーだけを控えるために使う。
 */
export async function findUsernamesByApi(
	request: APIRequestContext,
	usernames: ReadonlySet<string>
): Promise<string[]> {
	return (await listUsersForCleanup(request))
		.map(({ username }) => username)
		.filter((username) => usernames.has(username));
}

/** 後片付けや残したものの控えのためにユーザーの一覧を取る。取れなければ警告して空を返す。 */
async function listUsersForCleanup(request: APIRequestContext): Promise<AdminUserSummary[]> {
	try {
		return await listUsersByApi(request);
	} catch (err) {
		console.warn('ユーザーの一覧を取れませんでした:', err);
		return [];
	}
}

/** そのコンテキストでログインする。以降のページ遷移もログイン済みになる。 */
export async function logInByApi(
	request: APIRequestContext,
	username: string,
	password: string
): Promise<void> {
	await send(request, 'post', '/auth/login', { username, password });
}

/** ログインを試し、応答のステータスを返す。成功すると `request` のログイン状態はそのユーザーに替わる。 */
export async function loginStatus(
	request: APIRequestContext,
	username: string,
	password: string
): Promise<number> {
	const res = await request.post('/api/v1/auth/login', { data: { username, password } });
	return res.status();
}
