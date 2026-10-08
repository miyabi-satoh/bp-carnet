# アーキテクチャ

コードを読む前に知っておくと迷わない、全体の組み立てと横断的な仕組み。
開発の手順は DEVELOPMENT.md、ログインの仕組みは docs/authentication.md、iPhone・iPad のアプリは docs/mobile-app.md。

## 構成

- Backend: Rust + Axum 0.8 + SQLite (sqlx) + tower-sessions
- Frontend: SvelteKit の SPA (`ssr = false`、`adapter-static`)。Svelte 5 (runes)・Tailwind CSS 4・shadcn-svelte (`bits-ui`)・layerchart (グラフ)
- API は REST で、すべて `/api/v1` の下 (`src/api/mod.rs`)。`/api/v1` の下の未知のパスは SPA に落とさず 404 を返す。
- 型は backend から frontend へ一方向に流す。
  - `utoipa` で OpenAPI を生成し (`bp-carnet --openapi` → `openapi.json`)、`openapi-typescript` で `frontend/src/lib/api/schema.d.ts` を作る。
  - frontend は `openapi-fetch` の型付きクライアント (`frontend/src/lib/api/client.ts`) で呼ぶ。`just api-types-check` が生成物のずれを拾う。
- 配るものは実行ファイル1つ。`rust-embed` が `frontend/build/` を埋め込み (`src/static_files.rs`)、API 以外のパスは埋め込んだファイルか、無ければ `index.html` を返す。
  - `_app/immutable/` (ハッシュ付き) だけを長くキャッシュさせ、ほかは `no-cache`。
- 置き場所 (`src/config.rs` の `AppDirs`): 環境変数 `BP_CARNET_HOME` があれば、設定 (`config.toml`)・DB・セッション鍵・ログ・二重起動防止のロックをすべてそこに置く。無ければ OS 標準のディレクトリ。
- 設定は `config.toml` (項目と既定値は `config.example.toml`)。外部サービスの秘密の値は環境変数で渡す (`.env` も `dotenvy` が読む。`.env.example`)。

## リクエストが通る層

`src/lib.rs` の `build_app` で積む。外側 (先に受ける) から順に:

1. `forwarded::extract`: 信頼済みプロキシの伝えた scheme・host・接続元を解釈する (下の「リバースプロキシ配下」)。
2. CORS: モバイルアプリの出どころ (`capacitor://localhost`) だけを許す。preflight にはここで答える。
3. `app_version::require_supported`: 古い版のアプリの API 呼び出しを 426 で断る。
4. `csrf::same_origin_check`: GET 以外の別オリジンからのリクエストを 403 で断る。
5. セッション (`tower-sessions`、SQLite に保存) と、`__Host-` Cookie の削除を直す層。
6. gzip 圧縮、リクエストのログ (`TraceLayer`)。

## 機能の有効化条件

- 本番で必ず入る設定 (`[server] public_url`・`contact_url`、`[mail] host`・`from`) は必須で、空なら起動しない (`Config::check_required`)。
  - 未設定のときの画面を作らないため。開発とテストでは見本の値と手元のメールの受け口 (`just dev-mail`) を使う。
- 外部サービスの鍵は、ダミーの値では呼んだ時点で失敗するので、有無で機能ごと切る。起動時のログに有効・無効が出る (`src/main.rs`)。

| 機能                            | 有効になる条件                                                                                                                                              |
| ------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Google ログイン                 | `GOOGLE_OAUTH_CLIENT_ID`・`GOOGLE_OAUTH_CLIENT_SECRET`                                                                                                      |
| LINE ログイン                   | `LINE_LOGIN_CHANNEL_ID`・`LINE_LOGIN_CHANNEL_SECRET`                                                                                                        |
| Sign in with Apple (ウェブ)     | `APPLE_TEAM_ID`・`APPLE_KEY_ID`・`APPLE_PRIVATE_KEY`・`APPLE_SERVICE_ID` と、https の `[server] public_url`・`[session] secure_cookie = true`                |
| Sign in with Apple (アプリ)     | `APPLE_TEAM_ID`・`APPLE_KEY_ID`・`APPLE_PRIVATE_KEY`・`APPLE_APP_BUNDLE_ID`                                                                                 |
| 写真の読み取り (Gemini)         | `GEMINI_API_KEY` (→ docs/ocr.md)                                                                                                                            |
| OCR 枠の買い足し (Stripe)       | `[payments.stripe] enabled`・戻り先の URL と `STRIPE_*` (→ docs/payments.md)                                                                                |
| アプリ内課金 (App Store)        | `APP_STORE_ISSUER_ID`・`APP_STORE_KEY_ID`・`APP_STORE_PRIVATE_KEY`・`APPLE_APP_BUNDLE_ID` (→ docs/payments.md)                                              |
| 放置アカウントの自動退会        | `[inactivity] delete_after_days` が 0 より大きい                                                                                                            |
| 古い版のアプリに更新を促す      | `[mobile_app] min_version` (→ docs/mobile-app.md)                                                                                                           |
| 「オープンβテスト中」の表示     | `[server] beta_notice = true`                                                                                                                               |
| ログイン画面の紹介へのリンク    | `[server] intro_url` (ウェブだけ。アプリには出さない)                                                                                                       |

- 画面は、未ログインで叩ける `GET /api/v1/auth/providers` (ログイン方式・β表示) と `GET /api/v1/ocr/status` (読み取り・買い足し) で、どれが使えるかを知る。
- ログインには常にパスワードか外部の提供元の確認が要る。パスワード無しで入れるモードは無い。

## 検索エンジンへの見せ方

SPA の素の `index.html` はどのパスも同じ枠なので、`src/seo.rs` が `index.html` を返すときに `</head>` の直前へタグを差し込む (フロントのビルドは触らない)。

- 索引させるのは `/`・`/terms`・`/privacy`・`/help`・`/help/<slug>` だけ。title・description・canonical (`public_url` + パス)・OGP を出し、`/sitemap.xml` に載せる。
- それ以外 (ログインが要る画面・`/login`・存在しないパス) は `<meta name="robots" content="noindex">` と `X-Robots-Tag: noindex`。
  - 存在しないパスも 200 で `index.html` を返す。ルーティングはブラウザ側にあり、サーバーは有効なパスの一覧を持たないため。
  - `/login` は、未ログインの `/` がログイン画面になるのと重複するので索引させない。
- `robots.txt` は全許可で、Sitemap の行だけを足す。`/api/` を塞ぐと `X-Robots-Tag` が読まれなくなるので塞がない。
- 使い方の slug と title は `src/seo.rs` と `frontend/src/lib/help.ts`・`frontend/messages/ja.json` に二重に持つ。食い違うとテストが落ちる。
- title は、描画後の DOM でも効くよう、クライアント (`frontend/src/lib/page-title.ts`) でも同じ文言を出す。
- 記録の中身が検索に出ることは構造上ない。API は未ログインで 401 を返し、ログインが要る画面の HTML は中身の無い枠だけ。

## PWA

- Web App Manifest と Service Worker は `@vite-pwa/sveltekit` (`frontend/vite.config.ts`)。
- 登録は `frontend/src/routes/+layout.svelte` で、`window.isSecureContext` と `'serviceWorker' in navigator` が真のときだけ行う (アプリの中では登録しない)。
  - URL の文字列で判定しない。`127.0.0.1`・`*.localhost` などを取りこぼしやすいため。
- キャッシュするのは静的アセットだけ。API の応答と写真はキャッシュしない (共有端末でログアウトした後に、前の人の記録が残らないように)。
- `/api/` への画面遷移 (ログインのリダイレクト・CSV のダウンロード) は `index.html` に差し替えずサーバーへ届ける (`navigateFallbackDenylist`)。

## データの分離

- 記録・設定などは、どれもアカウントに1対1で結び付く。API はログイン中の利用者 (`AuthUser`、`src/auth.rs`) の ID で絞って読み書きする。
- ほかの人のデータを見られるのは管理者 (`AdminUser`、`/api/v1/admin/...`) だけ。

## CSRF

- GET 以外のリクエストは、`Origin` (無ければ `Referer`) の authority がリクエスト自身の host と一致しなければ 403 (`src/csrf.rs`)。
  - 自身の host は、信頼済みプロキシの伝えた host があればそれ、無ければ `Host`。
  - scheme は、信頼済みプロキシが proto を伝えたときだけ比べる。それが無いと、backend は接続が https かを知れないため。
  - `Origin`・`Referer` がどちらも無いリクエストは通す (ブラウザ以外のクライアントのため)。読めない値 (`Origin: null` など) は不一致として断る。
- 例外は2つ。モバイルアプリの出どころ (Cookie を使わずトークンで認証するため。→ docs/mobile-app.md) と、Apple のログインの戻り (Apple のサイトからの POST で、`state` の照合で守る)。
- セッション Cookie の `SameSite=Strict` だけでは同じサイトの別のホストを防げないので、このチェックで補う。

## リバースプロキシ配下

- `X-Forwarded-*` はクライアントが自由に名乗れるので、`[server] trusted_proxies` に挙げた接続元 (アドレスか CIDR) から来たときだけ採用する (`src/forwarded.rs`)。既定は空で、一切信頼しない。
- 読むヘッダーは `[server] proxy_headers` で選ぶ。
  - `"x-forwarded"` (既定): `X-Forwarded-Proto`・`-Host`・`-For`。受け取った値を消して付け直すプロキシ向け。`-Proto`・`-Host` は値がちょうど1つのときだけ使う。`-For` は末尾から信頼済みのアドレスを読み飛ばし、最初の信頼していないアドレスをクライアントとする。
  - `"fly"`: Fly.io の `Fly-Forwarded-Proto`・`Fly-Client-IP` だけを読む。fly-proxy はクライアントが送った `X-Forwarded-Proto`・`-Host` を書き換えずに渡すため。`Host` はそのまま届くので、転送先の host は読まない。
- ここで求めたクライアントのアドレスは、CSRF のログとレートリミットに使う。

## レートリミット

- プロセス内のメモリで数える (`AttemptRateLimiter`、`src/auth.rs`)。再起動や複数台をまたいだ制限はしない。
- 接続元は、IPv4 はアドレスごと、IPv6 は先頭 /64 ごとに数える (`forwarded::rate_limit_key`)。
- ログインは「接続元 + ユーザー名」・接続元・全体の3つで制限する。ユーザー名だけで数えると、第三者が本人の名前で試し続けて本人を締め出せるため。
- メールを送らせる申し込み (サインアップ・パスワードの再設定・メールアドレスの変更) は、接続元ごとと全体で制限する。
- 回数と窓は `AttemptRateLimiter` の各コンストラクタ (`for_logins` など) にある。

## ログ

- `tracing` に出す。出力先は `[log] output` (標準出力か、日ごとに分けたファイル)、絞り込みは `RUST_LOG` か `[log] filter` (`src/logging.rs`)。
- リクエストのログ (`TraceLayer`) にはパスだけを残し、クエリを含めない。メールのリンクのトークンのように、クエリに秘密の値が載ることがあるため。

## 表示言語 (i18n)

- 文言は Paraglide JS でキーにしてある (`frontend/messages/ja.json`、`frontend/project.inlang`)。今の言語は日本語だけ。
- 言語は、ブラウザに残した言語 (`localStorage`) → ブラウザの表示言語 → 日本語 の順で決める。最初に決まった言語が `localStorage` に残るので、あとでブラウザの表示言語を変えても追従しない。
