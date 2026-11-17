/** 管理 API に、確認コードを通してからの1時間が過ぎたと断られたか (docs/authentication.md「管理者の確認コード」)。
 * 立てば `admin-code-gate.svelte` が管理画面の代わりに入力を出す。画面を開いたときの確認だけで見ていると、
 * 開いたまま1時間を過ぎた画面の操作では、断りの文言が出るだけで入れ直す道が無いため。通したら下ろす。 */
export const adminVerification = $state({ expired: false });
