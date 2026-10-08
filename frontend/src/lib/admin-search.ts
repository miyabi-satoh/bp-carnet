/** 管理画面のユーザー一覧の絞り込み。ユーザー ID と名前 (Google・LINE の表示名) のどちらかに、
 * 打った文字が含まれるユーザーを残す。全角・半角と大文字・小文字は区別しない。 */
export function filterAdminUsers<T extends { username: string; displayName?: string | null }>(
	users: readonly T[],
	query: string
): T[] {
	const needle = normalize(query.trim());
	if (needle === '') return [...users];
	return users.filter(
		(user) =>
			normalize(user.username).includes(needle) ||
			(user.displayName != null && normalize(user.displayName).includes(needle))
	);
}

/** 照合にだけ使い、値は書き換えないので、NFKC で全角・半角をまとめて寄せる (`toHalfWidth` と違い、「㈱」などが崩れても困らない)。 */
function normalize(text: string): string {
	return text.normalize('NFKC').toLowerCase();
}
