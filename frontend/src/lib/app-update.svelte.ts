/** アプリの版が古く、サーバーに断られたか (docs/mobile-app.md)。立てばルートの layout が、どの画面の代わりにも
 * 更新を促す画面を出す。更新するまで API は使えないので、下ろさない。 */
export const appUpdate = $state({ required: false });
