// 設定やユーザーの状態を変えるテスト、集計の値を確かめるテストは、共有の管理者ではなく
// テストごとの使い捨てユーザーで行う。管理者や他のテストのデータに左右されず、残しもしないため
// (`E2E_KEEP_DATA=1` で残すときを除く、→ kept-data.ts)。
// テストからは fixture (`throwawayUser`、→ fixtures.ts) として使う。
import { randomUUID } from 'node:crypto';
import type { APIRequestContext } from '@playwright/test';
import { createUserByApi, logInByApi, removeAllRecordsByApi, removeUserByApi } from './api-helpers';
import { uniqueId } from './helpers';

export type ThrowawayUser = { id: number; username: string; password: string };

/**
 * パスワードの条件 (`[password]`) を満たすパスワード。環境ごとに条件が違うため、どの環境の
 * 最小長も超える長さで、数字・英小文字・英大文字・記号をすべて含める。
 */
export function uniquePassword(): string {
	return `Pw-${randomUUID()}`;
}

/**
 * 使い捨てユーザーを作り、`request` (テストの `page.request`) でログインする。ユーザー ID は、
 * 残ったときに管理画面で見分けられるよう `e2e-` で始める。
 * ログインに失敗したら、作ったユーザーを消してから投げ直す (fixture の後始末に届かないため)。
 */
export async function createThrowawayUser(
	adminRequest: APIRequestContext,
	request: APIRequestContext,
	testTitle: string
): Promise<ThrowawayUser> {
	const username = uniqueId('e2e-user');
	const password = uniquePassword();
	const id = await createUserByApi(adminRequest, username, password);
	try {
		await logInByApi(request, username, password);
	} catch (err) {
		await removeUserByApi(adminRequest, id);
		throw new Error(`使い捨てユーザーでログインできませんでした (${testTitle})`, { cause: err });
	}
	return { id, username, password };
}

/**
 * 使い捨てユーザーの記録を消してから、ユーザーを消す。後片付け専用で、失敗は警告に留める。
 * 記録はユーザーの削除で一緒に消えるが、猶予期間のあいだ残さないよう先に消す。記録を消すには
 * そのユーザーのログインが要るので、`newRequest` で作ったコンテキストで今のパスワードでログインし直す
 * (テストの `page` は閉じていることがあり、パスワードの変更で他のセッションは断たれるため)。
 */
export async function removeThrowawayUser(
	adminRequest: APIRequestContext,
	user: ThrowawayUser,
	newRequest: () => Promise<APIRequestContext>
): Promise<void> {
	try {
		const request = await newRequest();
		try {
			await logInByApi(request, user.username, user.password);
			await removeAllRecordsByApi(request);
		} finally {
			await request.dispose();
		}
	} catch (err) {
		console.warn(`使い捨てユーザーの記録を片付けられませんでした (${user.username}):`, err);
	}
	await removeUserByApi(adminRequest, user.id);
}
