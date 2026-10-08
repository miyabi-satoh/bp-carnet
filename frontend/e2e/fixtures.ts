// テストが作ったものを、テストが途中で落ちても片付けるための `test`。
//
// ADR: 後片付けはテスト本体の try/finally ではなく fixture の後始末で行う。テストがタイムアウトすると、
// 本体の finally は `page` などが閉じられた後に走り、API を呼べずに残してしまうため。
// 後始末は、テストの `page` とは別に作った API のコンテキストから呼ぶ。
// `E2E_KEEP_DATA=1` のときは片付けず、残したものを控えに書く (→ kept-data.ts)。
import { setTimeout } from 'node:timers/promises';
import { test as base, type APIRequestContext, type TestInfo } from '@playwright/test';
import {
	findRecordMemosByApi,
	findUsernamesByApi,
	removeRecordsByMemoByApi,
	removeUsersByUsernameByApi
} from './api-helpers';
import { ADMIN_STORAGE_STATE, ADMIN_USERNAME } from './auth';
import { KEEP_DATA, keptTestName, recordKept } from './kept-data';
import { createThrowawayUser, removeThrowawayUser, type ThrowawayUser } from './user-helpers';

export { expect } from '@playwright/test';

/** 未ログインの storageState。使い捨てユーザーで流すファイルで `test.use({ storageState: NO_LOGIN })` する。 */
export const NO_LOGIN = { cookies: [], origins: [] };

/**
 * 途中で落ちたテストは、送った作成がまだサーバーで終わっていないことがある。一覧を取る前に少し待つ。
 * 通ったテストは応答を待ち終えているので待たない。
 */
async function settleIfFailed(testInfo: TestInfo): Promise<void> {
	if (testInfo.status !== testInfo.expectedStatus) await setTimeout(2000);
}

export const test = base.extend<{
	/** 管理者のログイン状態の API。`page` のログイン状態とは別に持つ。 */
	adminRequest: APIRequestContext;
	/**
	 * 未ログインの API のコンテキストを作る。使い終えたら呼び出し側で `dispose` する。
	 * FIX: テストの中の `playwright.request.newContext` は設定の `storageState` を引き継ぐ。管理者の
	 * Cookie を持ったままログインすると、セッション ID が更新されて共有の管理者のセッションが切れるため、空を明示する。
	 */
	newLoggedOutRequest: () => Promise<APIRequestContext>;
	/**
	 * 管理者の記録のメモを控えると、テストの後にそのメモの記録を消す。記録を作る前に控える。
	 * id ではなくメモで探すのは、登録の応答が届く前にテストが終わっても取りこぼさないため。
	 */
	adminRecordCleanup: { add: (memo: string) => void };
	/**
	 * 管理者が画面から作るユーザーの ID を控えると、テストの後にそのユーザーを消す。作る前に控える (理由は同上)。
	 * パスワードは、残すとき (`E2E_KEEP_DATA=1`) に一覧へ出すために受け取る。
	 */
	adminUserCleanup: { add: (username: string, password: string) => void };
	/**
	 * 管理画面で操作する相手の使い捨てユーザー。`page` は管理者のまま、このユーザーのログイン状態は
	 * `request` に持つ (記録の準備やログインの確認に使う)。後片付けは `throwawayUser` と同じ。
	 */
	managedUser: ThrowawayUser & { request: APIRequestContext };
	/**
	 * 使い捨てユーザー。管理者の API で作り、`page` をそのユーザーでログインさせる。テストの後に、
	 * そのユーザーの記録を消してからユーザーを消す (削除の予約になる → removeUserByApi)。
	 * `page` は未ログインで始める (`NO_LOGIN`)。パスワードを変えたテストは `password` を書き換える。
	 */
	throwawayUser: ThrowawayUser;
}>({
	adminRequest: async ({ playwright, baseURL, ignoreHTTPSErrors }, use) => {
		const request = await playwright.request.newContext({
			baseURL,
			ignoreHTTPSErrors,
			storageState: ADMIN_STORAGE_STATE
		});
		await use(request);
		await request.dispose();
	},
	newLoggedOutRequest: async ({ playwright, baseURL, ignoreHTTPSErrors }, use) => {
		await use(() =>
			playwright.request.newContext({ baseURL, ignoreHTTPSErrors, storageState: NO_LOGIN })
		);
	},
	adminRecordCleanup: async ({ adminRequest }, use, testInfo) => {
		const memos = new Set<string>();
		await use({ add: (memo) => memos.add(memo) });
		if (memos.size === 0) return;
		await settleIfFailed(testInfo);
		if (KEEP_DATA) {
			// 登録に失敗させるテストや、テストの中で消す記録もあるため、実際に残ったものだけを控える。
			const test = keptTestName(testInfo);
			for (const memo of await findRecordMemosByApi(adminRequest, memos)) {
				recordKept({ kind: 'record', test, owner: ADMIN_USERNAME, memo });
			}
			return;
		}
		await removeRecordsByMemoByApi(adminRequest, memos);
	},
	adminUserCleanup: async ({ adminRequest }, use, testInfo) => {
		const users = new Map<string, string>();
		await use({ add: (username, password) => users.set(username, password) });
		if (users.size === 0) return;
		await settleIfFailed(testInfo);
		const usernames = new Set(users.keys());
		if (KEEP_DATA) {
			// テストが途中で落ちると作られていないことがあるため、実在するものだけを控える。
			const test = keptTestName(testInfo);
			for (const username of await findUsernamesByApi(adminRequest, usernames)) {
				recordKept({ kind: 'user', test, username, password: users.get(username) ?? '' });
			}
			return;
		}
		await removeUsersByUsernameByApi(adminRequest, usernames);
	},
	managedUser: async ({ adminRequest, newLoggedOutRequest }, use, testInfo) => {
		const request = await newLoggedOutRequest();
		try {
			const user = {
				...(await createThrowawayUser(adminRequest, request, testInfo.titlePath.join(' '))),
				request
			};
			await use(user);
			await keepOrRemoveThrowawayUser(adminRequest, user, newLoggedOutRequest, testInfo);
		} finally {
			await request.dispose();
		}
	},
	throwawayUser: async ({ page, adminRequest, newLoggedOutRequest }, use, testInfo) => {
		const user = await createThrowawayUser(
			adminRequest,
			page.request,
			testInfo.titlePath.join(' ')
		);
		await use(user);
		await keepOrRemoveThrowawayUser(adminRequest, user, newLoggedOutRequest, testInfo);
	}
});

/** テストの後の使い捨てユーザーを、残すときは控えに書き、そうでなければ片付ける。パスワードはテストが変えた後の値。 */
async function keepOrRemoveThrowawayUser(
	adminRequest: APIRequestContext,
	user: ThrowawayUser,
	newLoggedOutRequest: () => Promise<APIRequestContext>,
	testInfo: TestInfo
): Promise<void> {
	if (KEEP_DATA) {
		const { username, password } = user;
		recordKept({ kind: 'user', test: keptTestName(testInfo), username, password });
		return;
	}
	await removeThrowawayUser(adminRequest, user, newLoggedOutRequest);
}

/** すべてのテストを使い捨てユーザーで流すファイル向け。テストが `throwawayUser` を受け取らなくても用意する。 */
// 定義済みの fixture を auto に変える上書きはできないため、それを使う auto の fixture を足す。
export const userTest = test.extend<{ userSession: ThrowawayUser }>({
	userSession: [async ({ throwawayUser }, use) => use(throwawayUser), { auto: true }]
});
