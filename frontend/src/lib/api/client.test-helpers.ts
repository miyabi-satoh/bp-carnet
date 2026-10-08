// `client` (openapi-fetch) をモックする単体テストで共有する応答の作り方。

/** openapi-fetch の応答。2xx なら本文を `data` に、それ以外なら `error` に入れる。
 * 型付きのクライアントの戻り値を、テストでは必要な形だけで差し替えるため `any` で返す。 */
// eslint-disable-next-line @typescript-eslint/no-explicit-any
export function respond(status: number, body?: unknown): Promise<any> {
	const response = new Response(null, { status });
	return Promise.resolve(response.ok ? { data: body, response } : { error: body, response });
}
