import { client } from '$lib/api/client';
import { type ApiResult, ensureOkRequest, unwrapRequest } from '$lib/api/errors';
import type { components } from '$lib/api/schema';
import { userDisplayName } from '$lib/auth';
import * as m from '$lib/paraglide/messages.js';
import type { BpRecord, BpSummary } from '$lib/records';

export type AdminUser = components['schemas']['AdminUserResponse'];
export type AdminUserDetail = components['schemas']['AdminUserDetailResponse'];

/** 管理画面の一覧と閲覧ページで共通の、ユーザーの見出しと状態に要る項目。 */
type AdminUserHeading = Pick<
	AdminUserDetail,
	'displayName' | 'username' | 'role' | 'frozen' | 'deletionScheduledAt'
>;

/** 管理画面での見出し。管理者には「(管理者)」を付ける。 */
export function adminUserName(user: AdminUserHeading): string {
	const name = userDisplayName(user);
	return user.role === 'admin' ? m.admin_users_admin_name({ name }) : name;
}

/** 管理画面での状態の表示。削除予定は凍結より優先して出す: 凍結は戻せるが、こちらは猶予期間を
 * 過ぎると記録ごと消えるため、最初に気づいてほしい状態。 */
export function adminUserStatusLabel(user: AdminUserHeading): string {
	if (user.deletionScheduledAt) return m.admin_users_status_deletion_scheduled();
	return user.frozen ? m.admin_users_status_frozen() : m.admin_users_status_active();
}

/** ユーザーの一覧 (docs/authentication.md)。対象の定まらない取得なので、監査ログには残らない。 */
export async function fetchAdminUsers(): Promise<
	ApiResult<{ users: AdminUser[]; ocrDefaultBudgetYen: number }>
> {
	const result = await unwrapRequest(client.GET('/api/v1/admin/users'));
	return result.ok ? { ok: true, ...result.data } : result;
}

/** ユーザーのアカウント情報を取得する (docs/authentication.md)。取得のたびにサーバーが監査ログに記録する。 */
export async function fetchAdminUser(
	userId: number
): Promise<ApiResult<{ user: AdminUserDetail }>> {
	const result = await unwrapRequest(
		client.GET('/api/v1/admin/users/{id}', { params: { path: { id: userId } } })
	);
	return result.ok ? { ok: true, user: result.data } : result;
}

/** 利用者の記録を閲覧する (docs/authentication.md)。一覧と集計が1回の取得で揃う。
 * 取得のたびにサーバーが監査ログに記録する。 */
export async function fetchAdminUserRecords(
	userId: number,
	from: string,
	to: string
): Promise<ApiResult<{ records: BpRecord[]; summary: BpSummary }>> {
	const result = await unwrapRequest(
		client.GET('/api/v1/admin/users/{id}/records', {
			params: { path: { id: userId }, query: { from: from || undefined, to: to || undefined } }
		})
	);
	return result.ok ? { ok: true, ...result.data } : result;
}

/** ユーザーの凍結・解除 (docs/authentication.md)。自分自身や、有効な管理者が残らなくなる凍結は
 * サーバーが拒む。 */
export function setUserFrozen(userId: number, frozen: boolean): Promise<ApiResult> {
	return ensureOkRequest(
		client.PUT('/api/v1/admin/users/{id}/frozen', {
			params: { path: { id: userId } },
			body: { frozen }
		})
	);
}

/** ユーザーごとの OCR 累計金額 (円) の上限を変更する。`null` は設定の既定値、`-1` は無制限。 */
export function setUserOcrLimit(userId: number, ocrBudgetYen: number | null): Promise<ApiResult> {
	return ensureOkRequest(
		client.PUT('/api/v1/admin/users/{id}/ocr-limit', {
			params: { path: { id: userId } },
			body: { ocrBudgetYen }
		})
	);
}

/** ユーザーのパスワードを再設定する (docs/authentication.md)。対象のセッションはすべて断たれる。
 * ポリシーの判定はサーバーが行う。 */
export function resetUserPassword(userId: number, password: string): Promise<ApiResult> {
	return ensureOkRequest(
		client.PUT('/api/v1/admin/users/{id}/password', {
			params: { path: { id: userId } },
			body: { password }
		})
	);
}

/** ユーザーの削除を予約する (docs/authentication.md)。起点が管理者として記録されるため、
 * 本人のログインでは取り消されない (取り消しは `cancelUserDeletion`)。誤操作を防ぐ確認文字列の
 * 入力は画面側で行う。 */
export async function scheduleUserDeletion(
	userId: number
): Promise<ApiResult<{ deletionScheduledAt: string }>> {
	const result = await unwrapRequest(
		client.POST('/api/v1/admin/users/{id}/deletion', {
			params: { path: { id: userId } }
		})
	);
	return result.ok ? { ok: true, deletionScheduledAt: result.data.deletionScheduledAt } : result;
}

/** 管理者が起点の削除予約を取り消す (docs/authentication.md)。本人が起点の予約はサーバーが拒む。 */
export function cancelUserDeletion(userId: number): Promise<ApiResult> {
	return ensureOkRequest(
		client.DELETE('/api/v1/admin/users/{id}/deletion', {
			params: { path: { id: userId } }
		})
	);
}

/** ユーザーを作る (docs/authentication.md)。ユーザーIDの正規化・重複判定・パスワードの
 * ポリシー判定はいずれもサーバーが行う。 */
export async function createUser(
	username: string,
	password: string,
	role: 'user' | 'admin'
): Promise<ApiResult<{ username: string }>> {
	const result = await unwrapRequest(
		client.POST('/api/v1/admin/users', { body: { username, password, role } })
	);
	return result.ok ? { ok: true, username: result.data.username } : result;
}
