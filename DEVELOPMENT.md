# 開発

BP Carnet の開発の手順。Rust (axum) の backend が SvelteKit (SPA) の frontend を埋め込んで配信する。

## 技術スタック

- backend: Rust + axum
  - DB: SQLite (sqlx, WAL)、マイグレーションは `migrations/` を起動時に自動適用。クエリは
    `sqlx::query!`/`query_as!` 系マクロでコンパイル時チェックする (オフラインキャッシュ `.sqlx/` をコミット)
  - 設定: `config.toml` (`directories` で OS 標準のアプリデータディレクトリを解決、`BP_CARNET_HOME` で上書き可)
  - ロギング: tracing (stdout またはファイルへ日次ローテーション)
  - 認証: tower-sessions (署名付きセッション Cookie、SQLite ストア) + argon2 (パスワードハッシュ) + ログイン試行のレートリミット
  - エラー形式: `{"error":{"code","message"}}` の共通 envelope (`src/error.rs`)
  - OpenAPI 仕様生成: utoipa (`bp-carnet --openapi`)
  - frontend ビルド成果物は rust-embed で backend に埋め込む
- frontend: SvelteKit (SPA モード, `adapter-static`) + TypeScript
  - UI: shadcn-svelte (bits-ui) + Tailwind CSS v4
  - API クライアント: openapi-fetch + openapi-typescript (`openapi.json` から型生成)
  - テスト: vitest (unit)
- パッケージ管理: pnpm (frontend)

## セットアップ

コマンドランナーに [just](https://github.com/casey/just) を使う (`brew install just` または `cargo install just`)。

```sh
just install   # frontend の依存関係と、E2E・ブラウザテストが使う Chromium をインストール
```

push の前に整形 (`just fmt-check`) を確かめる pre-push フックに
[lefthook](https://github.com/evilmartians/lefthook) を使う (`brew install lefthook`)。
全部の検査 (`just ci`) は、GitHub Actions が PR ごとに流す。

```sh
just hooks-install   # pre-push フックをセットアップ
```

## 開発

```sh
just dev           # backend + frontend + メールの受け口をまとめて起動 (ラベル付きで1ターミナルに出力を集約)
just dev-backend   # backend だけ起動 (:3000, 初回のみ frontend をビルド)
just dev-frontend  # frontend だけ起動 (HMR 付き、/api は backend にプロキシ)
just dev-mail      # backend が送るメールを 127.0.0.1:1025 で受け、data/dev-runtime/mail/ に1通ずつ JSON で置く
```

- `BP_CARNET_HOME` に `data/dev-runtime/` がセットされる (git worktree の中でも元の clone のもの)。
- `dev-backend` は引数をそのまま bp-carnet 本体に渡す (例: `just dev-backend -v`)。
- 手元で `just ci` を流すと、`just dev-backend` と同じ debug の実行ファイルと `frontend/build` を作り直す (`e2e-local`)。動かしている dev-backend が落ちることがあるので、流す前に止める。
- `--create-user` は二重起動のロック (`data/dev-runtime/` を共有した2つ目の起動を止めるもの) より前に処理するので、サーバーが動いていても使える。

### モバイルアプリ (iPhone・iPad)

Xcode と iOS のシミュレータのランタイム (`xcodebuild -downloadPlatform iOS`) が要る。

```sh
just app-sync       # frontend をビルドして mobile/ios へ写す (呼び先は公開 URL)
just app-sync-dev   # シミュレータから開発用の backend につなぐ (Google・LINE は .env の開発用のもの)
```

写したあとは `mobile/ios/App/App.xcodeproj` を Xcode で開くか、`xcodebuild` でビルドする。
開発用の backend には `localhost` ではなく `127.0.0.1` でつなぐ (`localhost` では読み込めなかった)。

シミュレータで動いている Debug ビルドの画面は、`mobile/scripts/wir.py` で JS を評価して操作・観察できる (Web Inspector のソケットに直接つなぐ。追加のインストールは要らない)。

```sh
mobile/scripts/wir.py 'location.pathname'
mobile/scripts/wir.py '__h.click("印刷用レポート"); __h.wait(2000).then(() => document.body.innerText)'
xcrun simctl io booted screenshot shot.png   # 見た目はスクリーンショットで確かめる
```

- 使える関数 (`__h.click`・`fill`・`pick`・`go`・`wait`) は `mobile/scripts/wir-helpers.js`。
- iOS の写真・ファイルの選択画面、共有シート、印刷の画面は JS から操作できない。ファイルは `__h.pick` で選んだことにできる。渡すファイルは `mobile/ios/App/App/public/` に置けばアプリから読める (次の `cap sync` で消える)。
- Xcode 27 のシミュレータでは、座標でタップするツール (AXe 1.8.0) のタップが届かなかった (2026-09-25)。
- 起動して間もないシミュレータでは、`wir.py` が「時間切れ (app=None …)」で終わることがある。Web Inspector がアプリの一覧を返さないため。数分おいてから流し直すと通った (2026-10-05。その間に Safari の「開発」メニューでシミュレータを開いたので、どちらで通ったかは分けていない)。

### sqlx (コンパイル時クエリチェック)

DB アクセスは `sqlx::query!`/`query_as!` 系マクロで書いている。`just sqlx-prepare`
を実行したときに実際の DB へ接続して SQL を検証し、結果を `.sqlx/` (コミット対象) にオフライン
キャッシュする。通常のビルド・テスト (`just build`/`just test`/`just ci` 等) はこのキャッシュを
読むだけなので `.env` 無しで動く (実行時の dotenvy 読み込みも、ファイルが無ければ単に無視される)。
`.env` があるとマクロは DB を直接読むので、CI の `just ci` は `SQLX_OFFLINE=true` で流し
(`.github/workflows/ci.yml`)、キャッシュの再生成漏れを止める。

クエリを足す・変えるときは、開発用の DB にマイグレーションを当ててからキャッシュを作り直す:

```sh
mkdir -p "$(just --evaluate data_dir)/dev-runtime"
just db-migrate    # migrations/ を当てた開発用 DB を作る (新しいマイグレーションを足したときも)
just sqlx-prepare  # .sqlx/ を再生成する (クエリ変更のたびに実行してコミット)
```

作りたての DB では、先に `just dev-backend` を一度起動して止める。セッションの表 (`tower_sessions`) は
`migrations/` に無く、サーバーの起動時に作られる。無いと、それを読むテストのクエリで `sqlx-prepare` が落ちる。

どちらも `.env` は読まず、`just dev-backend` と同じ DB を渡す。`.env` の `DATABASE_URL` は、
ビルド (`just dev-backend`・`just test`・`cargo build` など) のときにマクロが読む。git worktree の中でも
ビルドするなら、絶対パスにする (worktree からも元の clone の `.env` が親のフォルダから見つかり、
相対パスは worktree から見たパスになる)。

キャッシュのハッシュは SQL 文字列由来のため、マイグレーションで列の型・NULL 制約だけを変えて
SQL 文字列自体は変えなかった場合、`.sqlx/` が古いままでもオフラインビルドはエラーにならない。
マイグレーションを変更したときも `just sqlx-prepare` を実行すること。

この DB (`data/dev-runtime/bp-carnet.db`) は `just dev-backend` の実行時 DB
(`BP_CARNET_HOME=data/dev-runtime`) と共用する (開発時に触るDBを1つにするため)。
git worktree の中でも、`just` のコマンドは元の clone の `data/` を使う (`justfile` の `data_dir`)。
マイグレーションファイルの追加は `cargo sqlx migrate add <name>`。sqlx-cli 自体の
インストールは `cargo install sqlx-cli --no-default-features --features sqlite`。

開発用の DB は使い捨てでよい。次のときは、DB を消して `just db-migrate` で作り直す。

- マイグレーションを含むブランチを取り下げたとき。適用の記録だけが残り、main の起動が `was previously applied but is missing in the resolved migrations` で落ちる。
- 適用した後にマイグレーションのファイルを直したとき。起動が `was previously applied but has been modified` で落ちる。作り直したくなければ、`_sqlx_migrations` のその行の `checksum` を、ファイルの SHA-384 に直す。

## テスト

```sh
just test        # テスト実行 (cargo test + vitest)
just e2e <URL> <管理者パスワード> [管理者ID] # 起動済みの環境に E2E (Playwright) を流す
just e2e-local   # 使い捨ての backend (管理者1人) と、メールを受け取る SMTP の受け口を立てて E2E を流す
```

## ビルド

```sh
just build  # frontend をビルドしてから release バイナリをビルド
```

## デプロイ

bp.amiiby.com を Fly.io の東京リージョン (`nrt`) で動かす手順。

- 初回セットアップ: 1〜7 を上から順に進めれば公開できる。
- 運用: 更新・復元などを、必要なときに引く。

### 構成

- Fly.io の Machine 1台と Volume 1つ (`/data`) で動かす。HTTPS の終端と証明書は Fly.io のプロキシ (fly-proxy) が受け持つ。
- DB (SQLite) は Litestream で Cloudflare R2 へ常に複製する。7日前まで戻せる。
  - Volume は1台のサーバーのディスクにあり、そのサーバーが壊れると失われるため。
- app は2つ。同じイメージを載せる。
  - 本番 (`deploy/cloud/fly.toml`)
  - 検証用 (`deploy/cloud/fly.staging.toml`): 本番の前に載せて確かめる。アクセスでは起きないので、使うときに `just staging-start` で起こす。
  - 以下の `<本番の app>`・`<検証用の app>` は、それぞれの `fly.toml` の `app`。
- どちらの app も、しばらく通信が無いと Machine が一時停止する (`auto_stop_machines = "suspend"`)。本番は次のアクセスで再開し、1日に1回、起動し直す (→「毎日の再起動」)。
- イメージは Docker Desktop で作り、`registry.fly.io` へ送る。amd64 の Windows でも、Apple Silicon の Mac でもよい。
  - 出来るイメージは同じ amd64 (`just cloud-image` が `--platform linux/amd64` を付ける)。
  - Apple Silicon の Mac は amd64 をエミュレーションするため、初回は遅い (10分を超えた)。キャッシュが効く2回目以降は短い。
  - frontend と Rust のビルドは `deploy/cloud/Dockerfile` の中で行う。
- `deploy/cloud/` のファイル:
  - `config.production.toml`・`config.staging.toml`: アプリの設定。イメージに入れ、起動のたびに `start.sh` が `/data/config.toml` へ写す。
  - `litestream.yml`: 複製の設定。Litestream が bp-carnet を子プロセスとして起動する。
  - `start.sh`: コンテナの入口。Volume の所有者を直し、UID 10001 に切り替えて Litestream を起動する。
- API キーなどの秘密は Fly のシークレット (環境変数) で渡す。

### 初回セットアップ

#### 1. ビルドするマシンの準備

- Docker Desktop、just (1.44 以上) を入れる。
- flyctl を入れ、ログインする (ブラウザが開く)。
  ```sh
  fly auth login
  ```

#### 2. Cloudflare R2 の準備

1. Cloudflare で R2 を有効にし、バケットを2つ作る。Location は Asia-Pacific (`apac`) を選ぶ。
   - 場所は保証されない (優先するだけ)。プライバシーポリシーの「外的環境の把握」に、国を特定できないことと、この指定を書いている。
   - 本番用と検証用。名前は `fly.toml`・`fly.staging.toml` の `R2_BUCKET` に合わせる。
2. R2 の「API Tokens」で、権限「Object Read & Write」をそれぞれのバケットだけに絞ったトークンを1つずつ作る。
   表示された Access Key ID と Secret Access Key はその場でしか見られないので、5. のシークレットにすぐ入れる。
   - バケットとトークンを分けるのは、検証用の Machine から本番の複製を書き換えられないようにするため。
3. アカウント ID (R2 の Overview に出る) を控える。

#### 3. Gemini API の準備

写真の読み取り用。5. のシークレット `GEMINI_API_KEY` に入れる API キーを用意する。
アプリの上限は1人ごとの累計の金額 (無料枠。5. のシークレット `OCR_FREE_BUDGET_YEN`) で、全員を合わせた上限は無い。同じ人の読み取りは1件ずつにする (→ docs/ocr.md)。
費用の歯止めは Google 側で掛ける。

本番と検証用で Google Cloud のプロジェクトと API キーを分ける (支出の上限と回数の上限はプロジェクト単位のため)。

1. 有料枠にする。AI Studio で Cloud Billing のアカウントを紐付け、前払いする。
   - 無料枠では、送った写真と結果が Google の製品の改善に使われ、人が読むことがある。健康データなので、本番では必ず有料枠にする。
2. AI Studio の Spend のページで、プロジェクトの月の支出上限 (Monthly spend cap) を設定する。
   - 上限に達すると、その月の残りは全員が読み取れなくなる (数値の入力による記録はできる)。
   - 課金の反映が約10分遅れるので、その分は超えることがある。
3. 前払いの自動チャージは使わないか、使うなら月の上限を付ける。
4. Cloud Billing の予算とアラート (Budgets & alerts) を、Gemini API に絞って作る (例: 50%・90%・100% で通知)。アラートは通知するだけで、止めはしない。
5. API キーに「Gemini API だけ」の制限を付ける。

#### 4. メール送信 (Resend) の準備

確認メール・パスワード再設定メールを送る。本番・検証用ともに設定する (検証用も本番と同じ送信元。→ `deploy/cloud/config.staging.toml` の `[mail]`)。

- プラン: 無料 (月3,000通・日100通、カード不要)。足りなくなったら `[mail]` の SMTP の設定を変えるだけで Amazon SES などに移れる。
- 送信元: `BP Carnet <noreply@bp.amiiby.com>`。
  - ドメインは `amiiby.com` 全体の評判と切り離すため、Resend の推奨どおりサブドメイン `bp.amiiby.com` にした。リージョンは東京 (ap-northeast-1)。
- `amiiby.com` の DNS は Cloudflare。Resend の Domains の **Records** に出る値を、そのまま Cloudflare に足す。

  | TYPE | ホスト名 | 役割 |
  | --- | --- | --- |
  | TXT | `resend._domainkey.bp` | DKIM の公開鍵 (値は Resend の画面から。大文字小文字まで一致させる) |
  | CNAME | `send.bp` | 送信 (`send.forge.rmta.net`) |
  | CNAME | `rsend.bp` | 送信 (`rsend-apne1.forge.rmta.net`) |

  - DMARC は `bp` 用のレコード (`_dmarc.bp`) を置いていない。`amiiby.com` の `v=DMARC1; p=none;` が `bp.amiiby.com` にも効く (2026-09-29 に dig で確認)。`p=none` は検査に落ちたメールも弾かない。
  - `bp` 自体は `amiiby.com` への CNAME だが、足すのは `bp` の下の別の名前なので同居できる。
  - 認証は Resend の Domains で **Verify** を押す。通らなければ DNS の値を見直して **Restart verification**。
- API キー: Resend の **API Keys** で **Sending access**、ドメインを `bp.amiiby.com` に絞って発行する。表示は1回だけなので、パスワード管理ソフトに保存する (リポジトリ・チャットには貼らない)。
  - 5. の本番・検証用それぞれの `SMTP_PASSWORD` (Fly のシークレット) に入れる。
- 出典: [ドメインの追加](https://resend.com/docs/add-a-domain)・[認証が通らないとき](https://resend.com/docs/knowledge-base/what-if-my-domain-is-not-verifying)・[API キー](https://resend.com/docs/create-an-api-key)・[DMARC](https://resend.com/docs/dashboard/domains/dmarc)

#### 5. Fly.io の app を作る

本番と検証用のそれぞれで、`<app>` を `<本番の app>`・`<検証用の app>` に読み替えて実行する。

```sh
fly apps create <app>
fly volumes create bp_data --app <app> --region nrt --size 1
```

- Volume が1つだけの構成になるため、`fly volumes create` は確かめのプロンプトを出す。1台で動かす前提 (→「構成」) なので、そのまま進める。
- Volume のスナップショットの保持日数は、`fly.toml` の `[mounts]` の `snapshot_retention` (本番 7日、検証用 1日) に揃う。

シークレットを入れる。値がコマンドの履歴に残らないよう、`fly secrets import` に標準入力から `NAME=VALUE` の行で渡す (Windows では入力の終わりに Ctrl+Z → Enter)。

```sh
fly secrets import --app <app>
```

```text
LITESTREAM_ACCESS_KEY_ID=...
LITESTREAM_SECRET_ACCESS_KEY=...
R2_ACCOUNT_ID=...
GEMINI_API_KEY=...
```

本番だけ、次も入れる。

```text
GOOGLE_OAUTH_CLIENT_ID=...
GOOGLE_OAUTH_CLIENT_SECRET=...
LINE_LOGIN_CHANNEL_ID=...
LINE_LOGIN_CHANNEL_SECRET=...
APPLE_TEAM_ID=...
APPLE_KEY_ID=...
APPLE_PRIVATE_KEY=...
APPLE_SERVICE_ID=...
APPLE_APP_BUNDLE_ID=...
APP_STORE_ISSUER_ID=...
APP_STORE_KEY_ID=...
APP_STORE_PRIVATE_KEY=...
```

検証用も含め両方に、次を入れる。Stripe は、本番は本番アカウント、検証用はサンドボックスのものを入れる。

```text
SMTP_PASSWORD=...
STRIPE_SECRET_KEY=...
STRIPE_WEBHOOK_SECRET=...
STRIPE_OCR_TOPUP_PRICE_ID=...
STRIPE_TAX_RATE_ID=...
OCR_FREE_BUDGET_YEN=...
OCR_TOPUP_GRANT_YEN=...
OCR_INPUT_USD_PER_MILLION_TOKENS=...
OCR_OUTPUT_USD_PER_MILLION_TOKENS=...
OCR_JPY_PER_USD=...
```

- `OCR_*` は写真の読み取りの無料枠・買い足し1回の量・Gemini の単価 (→「写真の読み取りの無料枠・単価を変える」)。`GEMINI_API_KEY` があるのに無料枠か単価が無い、または Stripe かアプリ内課金が有効なのに買い足しの量が無いと、アプリが起動しない。
- `SMTP_PASSWORD` は Resend の API キー (→ 4.)。`config.production.toml`・`config.staging.toml` どちらも `[mail]` に `username` があるため、無いとアプリが起動しない。
- Stripe (OCR 枠の買い足し。→ docs/payments.md):
  - 製品の設定: Product (目印 `metadata[product]=bp-carnet`・税コード)、300円の Price (`tax_behavior` は `inclusive`)、Webhook の送り先 `https://<ホスト>/api/v1/payments/stripe/webhook` (`checkout.session.completed`・`checkout.session.async_payment_succeeded`・`checkout.session.async_payment_failed`・`charge.refunded`・`charge.dispute.created` を送らせる)。
  - アカウント全体の設定: 手動の税率「消費税 10%・税込み」、「送信メール」の「決済成功時」(購入者に領収書を送る)。
  - Price の ID を `STRIPE_OCR_TOPUP_PRICE_ID` に、Webhook の署名のシークレットを `STRIPE_WEBHOOK_SECRET` に、税率の ID を `STRIPE_TAX_RATE_ID` に入れる。税率は課税事業者になる 2026-12-01 から Checkout に付け、無いとその日から買い足しを止める (起動ログに警告が出る)。
  - Managed Payments は使わない (→ docs/payments.md)。Checkout Session ごとに外すので、同じアカウントのほかの製品が使っていても効かない。
  - シークレットがそろい、`config.*.toml` の `[payments.stripe]` の `enabled = true` のときだけ有効になる (起動ログに「OCR 枠買い足し (Stripe) 機能が有効です」)。
- Google Cloud Console の OAuth クライアントには、承認済みのリダイレクト URI として `https://bp.amiiby.com/api/v1/auth/google/callback` を登録する。
- LINE Developers の LINE ログインチャネルには、コールバック URL として `https://bp.amiiby.com/api/v1/auth/line/callback` を登録する。
  - チャネルシークレットは、保存した LINE のトークンの暗号の鍵も兼ねる。発行し直すと、それまでのトークンでは連携を取り消せなくなる (次に LINE でログインした人の分から戻る)。
  - チャネルが「開発中」の間は、チャネルの Admin・Tester だけがログインできる。
- Apple Developer では、アプリの App ID で Sign in with Apple を有効にし (primary)、ウェブのサービス ID をそこにまとめる。サービス ID の Return URL に `https://bp.amiiby.com/api/v1/auth/apple/callback` を登録する (docs/authentication.md)。
  - `APPLE_PRIVATE_KEY` は、Sign in with Apple を有効にした鍵 (.p8) の中身。保存した Apple のトークンの暗号の鍵も兼ねるので、鍵を作り直すと、それまでのトークンでは取り消せなくなる。
  - 転送用アドレス (`@privaterelay.appleid.com`) へメールを届けるため、送信元のドメインを「Private Email Relay Service」に登録する。
- アプリ内課金 (docs/payments.md) は、App Store Connect で次を用意する。`APP_STORE_*` と `APPLE_APP_BUNDLE_ID` がそろったときだけ有効になる (起動ログに「アプリ内課金 (App Store) が有効です」)。
  - アプリ内課金の鍵を作り、Issuer ID・キー ID・鍵 (.p8) の中身を `APP_STORE_ISSUER_ID`・`APP_STORE_KEY_ID`・`APP_STORE_PRIVATE_KEY` に入れる。Sign in with Apple の鍵とは別のもの。
  - App Store Server Notifications (V2) の URL を、本番・サンドボックスとも `https://bp.amiiby.com/api/v1/payments/apple/notifications` にする。送り元が Apple の範囲 (17.0.0.0/8) でない通知は 403 で断るので、`[server] proxy_headers` で本当の接続元が取れている必要がある。
- 検証用は Google ログインは試さない。

#### 6. 検証用に載せて確かめる

リポジトリのルートで実行する。

```sh
just deploy-staging
```

- イメージを作り (`just cloud-image`)、`registry.fly.io/<検証用の app>:<コミット>` として送り、`fly deploy --image` で載せる。
- 止まっていた Machine は、載せた後も止まったまま。この節の確かめの前に `just staging-start` で起こす。
- 起動したかはログで確かめる。
  ```sh
  fly logs --app <検証用の app>
  ```

##### 接続元を確かめる

初回に行う。Fly.io の構成が変わったと感じたときにも、やり直す。

`trusted_proxies = ["172.16.0.0/12"]` は、fly-proxy が Machine へつなぐ接続元のアドレスに合わせてある。
これは Fly.io の社員のフォーラムでの回答だけが根拠なので、実際の接続元を見て確かめる。

ブラウザで `https://<検証用の app>.fly.dev` を開いたまま、Machine の中で TCP の接続の一覧を見る。

```sh
fly ssh console --app <検証用の app> -C "cat /proc/net/tcp"
```

- 3000 番 (16進で `0BB8`) への接続の相手のアドレスが、`172.16.0.0/12` (172.16.x.x〜172.31.x.x) に入っていることを見る。
  アドレスは16進・リトルエンディアンで出る (例: `0A0010AC` は 172.16.0.10)。
- 外れていたら、ログインの試行回数の制限が全員まとめて数えられる (接続元を取り違える)。`config.*.toml` の `trusted_proxies` を直す。

##### ヘッダーを詐称できないか確かめる

初回だけ行う。

`Fly-Client-IP` と `Fly-Forwarded-Proto` を、利用者が送った値で上書きできないことを確かめる。
`Fly-Forwarded-Proto` は Fly.io の公式ドキュメントのヘッダーの一覧に無く、付くことは実測だけが根拠のため (→ docs/architecture.md)。

```sh
curl -sS -H "Fly-Client-IP: 192.0.2.1" -H "Fly-Forwarded-Proto: http" -H "Origin: http://<検証用の app>.fly.dev" -H "Content-Type: application/json" -d "{}" https://<検証用の app>.fly.dev/api/v1/auth/login
```

- 403 (`csrf_rejected`) なら、`Fly-Forwarded-Proto` は fly-proxy の値 (`https`) で上書きされている。400 などそれ以外なら、詐称が通っているか、fly-proxy がこのヘッダーを付けていない。
- `Fly-Client-IP` は `fly logs` では見えないので、`Fly-Client-IP` を付けたままログインを試行の上限まで失敗させ、別の回線 (スマホのモバイル通信など) からはログインの試行が止められないことで確かめる。
  詐称が通るなら、同じ値を送った全員がまとめて数えられる。
- どちらかが通っていたら、公開の前に `src/forwarded.rs` の Fly.io の読み方を見直す。

##### 管理者を作る

最初の管理者は、Machine の中から `--create-user` で作る。パスワードは対話で入力する。
```sh
fly ssh console --app <app> --pty -C "setpriv --reuid=10001 --regid=10001 --clear-groups env BP_CARNET_HOME=/data bp-carnet --create-user <ユーザー ID> --admin"
```

- `setpriv` でアプリと同じ UID にするのは、root のまま DB に書くと、アプリが DB の一時ファイルを読めなくなるため。
- 本番では、ユーザー ID を Google アカウントのメールアドレスにしておくと、Google でログインしたときに同じアカウントとして統合される。

##### E2E を当てる

Mac から、検証用の管理者で流す。検証用は 512MB なので、Playwright の既定の同時数だと OOM で強制終了される。`--workers=2` を付ける。
パスワードはコマンドの引数に書かず、環境変数で渡す (`ps` に出るため)。

```sh
cd frontend
export E2E_BASE_URL=https://<検証用の app>.fly.dev E2E_ADMIN_USER=<管理者のユーザー ID> E2E_ADMIN_PASSWORD=<管理者のパスワード>
pnpm exec playwright test --workers=2
```

- `pwa.e2e.ts`「Service Worker が有効になり…」は、起動直後の Machine (512MB) だと 5 秒以内に有効にならず失敗することがある。本番の Chrome の DevTools で Service Worker が有効なら、時間がかかっただけと見てよい。

##### 複製を確かめる

R2 のダッシュボードで、検証用のバケットにファイルが増えていることを見る。
復元も試すときは、運用「複製と復元」の手順を検証用で行う。

#### 7. 本番に載せる

```sh
just deploy
```

初回だけ、続けて次を行う。

1. 管理者を作る (→ 6.「管理者を作る」、`<app>` は `<本番の app>`)。公開 (DNS を向ける) より先に作る。
2. 証明書を作る。
   ```sh
   fly certs add bp.amiiby.com --app <本番の app>
   ```
   表示された DNS のレコード (AAAA・A、または `_acme-challenge` の CNAME) を控える。
3. amiiby.com の DNS (Cloudflare) に、表示されたレコード (種別・名前・値。`_acme-challenge` などがあればそれも) をそのまま足す。
   - Cloudflare のプロキシ (オレンジの雲) は使わず、DNS のみにする。証明書の発行と接続元の判定 (`Fly-Client-IP`) が単純なままになるため。
4. `fly certs show bp.amiiby.com --app <本番の app>` で発行を確かめ、`https://bp.amiiby.com` を開く。

公開後に確かめる。

- `https://bp.amiiby.com/sitemap.xml` と `/robots.txt` (Sitemap の行) を開く。
  Search Console に `bp.amiiby.com/sitemap.xml` を送る。
- メール: 自分のアドレスでサインアップして確認メールが届く (迷惑メールに入らない) こと。docomo などの携帯のアドレスでも1度試す。

「オープンβテスト中」の表示は、`deploy/cloud/config.production.toml` (検証用も同様) の `beta_notice` で出し分ける。今は `false`。

### 運用

#### 更新

検証用で確かめてから本番に載せる。

```sh
just deploy-staging
just staging-start   # 止まっていた Machine は、載せた後も止まったまま
just deploy
```

- DB のマイグレーションは起動時に自動で適用される。
- 載せ替えの間 (数十秒) は止まる。Volume が1つなので、新旧を並べて切り替えられない。
- `fly secrets set` や `fly secrets import` でシークレットを変えても、Machine が起動し直す。

#### 毎日の再起動

GitHub Actions (`.github/workflows/daily-restart.yml`) が、毎日 3時すぎ (日本時間) に本番の Machine を起動し直す。

- 起動したときと、その後24時間ごとに走る処理が2つある。一時停止の間は24時間を数えるタイマーが進まないので、起動し直さないと2回目以降が何日も来ない。
  - 期限を過ぎたアカウントなどの掃除 (`src/account.rs` の `purge_expired_periodically`)。
  - Litestream の日ごとのスナップショット。間隔が延びると、戻せる期間が7日より短くなる。
- メンテナンスモードの作業 (→「ある時点へ戻す」) が3時すぎにかかるときは、先に `gh workflow disable daily-restart.yml` で止め、終わったら `gh workflow enable daily-restart.yml` で戻す。
- GitHub のシークレット `FLY_API_TOKEN` に、本番の app だけを操作できるトークンを入れる。
  ```sh
  fly tokens create deploy --app <本番の app> --name "GitHub Actions daily-restart" | gh secret set FLY_API_TOKEN --repo <リポジトリ>
  ```
- 失敗すると、GitHub からメールが届く。手で流すときは `gh workflow run daily-restart.yml`。
- GitHub は、リポジトリに60日動きが無いと定期実行を止める。止まったら、Actions の画面で有効に戻す。

#### 複製と復元

- 変更は1秒ごとに R2 へ送られる。日ごとのスナップショットを7日分残し、それより古い分は消える (`litestream.yml`)。
- Fly.io の Volume のスナップショットも7日分残る (`fly.toml` の `snapshot_retention`)。
- 消した記録や削除したアカウントは、複製とスナップショットに、おおむね8日残る (スナップショットの間隔 24時間 + 保持 7日)。プライバシーポリシーにこの日数を書いているので、保持の日数を変えるときはポリシーも直す。
  - DB は `secure_delete` で消した行を上書きするので、空きページにも残らない (`src/db.rs`)。効くのは設定した後に消した行だけ。本番の DB は Fly.io で新しく作るので問題ないが、既存の DB を移すときは、先に `VACUUM` で空きページを消しておく。
- `[ocr] dump_dir` (Gemini とのやりとりを残す設定) は本番では設定しない。写真を保存しないとプライバシーポリシーに書いているため。
- セッションの署名鍵 (`/data/session.key`) は複製しない。Volume を失うと作り直され、全員がログインし直すことになる。
- Volume を失った (Machine を作り直した) ときは、起動時に複製から自動で戻す (`restore-if-db-not-exists`)。この動作は、Fly.io の検証用でも確かめてある。

##### Volume を失ったときの復元を試す

検証用で行う。`<app>` は `<検証用の app>`。

**手順 3 は取り消せない (Machine と Volume を削除する)。app 名が検証用であることを確かめてから実行する。**

1. Machine と Volume の ID を控える。
   ```sh
   fly machines list --app <app>
   fly volumes list --app <app>
   ```
2. Machine を止めて、最後の同期を待つ。
   ```sh
   fly machine stop <machine-id> --app <app>
   ```
3. Machine と Volume を削除する。取り消せない。
   ```sh
   fly machine destroy <machine-id> --app <app> --force
   fly volumes destroy <volume-id> --app <app> --yes
   ```
4. 空の Volume を作る (`fly deploy` は Volume を自動で作らない)。
   ```sh
   fly volumes create bp_data --app <app> --region nrt --size 1 --yes
   ```
5. 既存のイメージで載せ直す。
   ```sh
   fly deploy --image registry.fly.io/<app>:<タグ> --config deploy/cloud/fly.staging.toml --ha=false --app <app>
   ```
6. Machine が止まっていたら `just staging-start` で起こす。`fly logs` に `attempting restore before replication` → `restore completed` が出て、復元した DB で起動し、複製が再開することを確かめる。

##### ある時点へ戻す

DB を差し替える間は、メンテナンスモード (`deploy/cloud/start.sh`) でアプリと複製を止めておく。
Machine を止めると `fly ssh console` で入れず、中で DB を消して起動し直すと最新の複製から戻ってしまうため。

1. メンテナンスモードに入る。Machine が起動し直し、アプリも複製も動かずに待つ。

   ```sh
   fly secrets set MAINTENANCE=1 --app <app>
   ```

2. Machine に入り、以降は中で実行する。

   ```sh
   fly ssh console --app <app>
   U="setpriv --reuid=10001 --regid=10001 --clear-groups"
   ```

3. 戻せる時点を見る。DB のファイル名は改名前の `bp-tracker.db` のまま (複製先も同じ)。以下のコマンドも `bp-tracker.db` で書いてある。
   - Volume も複製先も空の新しい環境は例外。アプリが `bp-carnet.db` を作り、`bp-tracker.db` の複製の対象から外れる。その環境の `litestream.yml` は `bp-carnet.db` にし、以下も `bp-carnet.db` に読み替える。

   ```sh
   $U litestream ltx -config /etc/litestream.yml -level all /data/bp-tracker.db
   ```

4. 戻したい時点 (RFC3339、例: `2026-09-17T13:53:30Z`) の DB を別の名前で作り、差し替える。

   ```sh
   $U litestream restore -config /etc/litestream.yml -timestamp <時点> -o /data/restored.db /data/bp-tracker.db
   rm -f /data/bp-tracker.db-wal /data/bp-tracker.db-shm
   mv /data/restored.db /data/bp-tracker.db
   ```

5. 複製の続き具合の記録を消す。これをしないと、差し替えた DB を古い複製の続きとして扱ってしまう。

   ```sh
   $U litestream reset -config /etc/litestream.yml /data/bp-tracker.db
   ```

6. Machine から出て、メンテナンスモードを抜ける。Machine が起動し直し、戻した状態から複製が続く。

   ```sh
   fly secrets unset MAINTENANCE --app <app>
   ```

注意:

- メンテナンス中はアプリが応答しないので、ヘルスチェックが落ち、画面も開けない。
  - 1 の `fly secrets set` は、ヘルスチェックを待って時間切れのエラーで終わることがある。Machine が起動し直していれば、メンテナンスモードには入っている (`fly logs` に「MAINTENANCE=1 のため」が出る)。
- `fly secrets set MAINTENANCE=1` が時間切れになっても、Fly.io 側の処理は続き、Machine のリースが残る。その間の `fly secrets unset` は「lease currently held」で失敗するので、リースの期限 (最長で数分) を待ってからやり直す。
- 通信が無いと Machine が一時停止する (`auto_stop_machines`。本番と検証用の両方)。一時停止したら `fly machine start <machine-id> --app <app>` (検証用は `just staging-start`) で再開し、途中だった手順からやり直す。
  - 4 の途中で止まったら、`rm -f /data/restored.db*` で作りかけの DB を消してから 4 をやり直す。
  - 5 を済ませる前に 6 へ進まない。
- 戻した DB の権限は、アプリが起動時に 600 に直す (`src/db.rs`)。

#### 前の版に戻す

戻したい版のコミットのタグを指定して載せ直す。送ったイメージは `registry.fly.io` に残っている。

```sh
fly releases --app <app> --image
fly deploy --ha=false --config deploy/cloud/fly.toml --image registry.fly.io/<app>:<戻したい版のタグ>
```

- 検証用では `--config deploy/cloud/fly.staging.toml` にする。止まっていた Machine は載せた後も止まったままなので、`just staging-start` で起こす。

マイグレーションを含む版から戻す場合は、DB も更新前の時点に戻す必要がある (→「ある時点へ戻す」)。

#### 稼働・利用状況を見る

```sh
export BP_REPORT_USER=<管理者のユーザー ID> BP_REPORT_PASSWORD=<パスワード>
just report              # 本番
just report --no-wake    # Fly の状態だけ。Machine を起こさない
just staging-start && just report --url https://<検証用の app>.fly.dev --app <検証用の app>   # 検証用 (アクセスでは起きない)
```

- 出るもの:
  - Fly: Machine の状態、起動・停止の回数、最後の終了。
  - アプリ: 起動を待った時間、確認済みのユーザー数、最終アクセスごとの人数、OCR の使用額、削除予約。
- **アプリへの問い合わせは Machine を起こす** (本番だけ。検証用は起きない)。 止まっている間の回数を数えたいときは、先に `--no-wake` で見る。
- パスワードは環境変数で渡す (引数に書くと `ps` に出るため)。管理者は ID/PW でログインできる必要がある (Google 専用のアカウントでは使えない)。
- 日ごとの推移 (新規登録・記録件数) は出ない。管理 API が返さないため。要るときは、集計用の管理 API を足す。
- Fly の Event Logs は直近の数件しか残らないので、起動・停止の回数は「見えた範囲」の数。

#### DB の中を見る

Machine に入っている `sqlite3` で、読み取り専用で開く。DB のファイル名は `bp-tracker.db`。Volume も複製先も空の新しい環境では `bp-carnet.db` に読み替える (→「ある時点へ戻す」の 3.)。

```sh
fly ssh console --app <app> -C "sqlite3 -readonly /data/bp-tracker.db 'select version, success from _sqlx_migrations order by version desc limit 3;'"
```

- 手で書き換えない。アプリの確かめを通らない変更になる。直すのはアプリ・管理画面・マイグレーションで行う。
- どうしても書くときは、`setpriv --reuid=10001 --regid=10001 --clear-groups` を前に付けてアプリと同じ UID で開く (root で書くと、アプリが DB の一時ファイルを読めなくなる)。
- Machine が止まっていると入れない。先に `fly machine start <machine-id> --app <app>` (検証用は `just staging-start`) で起こす。

#### 写真の読み取りの無料枠・単価を変える

無料枠・買い足し1回の量・Gemini の単価は、本番と検証用のそれぞれのシークレットに置く。`fly secrets set` は Machine を入れ替えて反映する。

```sh
fly secrets set --app <app> OCR_INPUT_USD_PER_MILLION_TOKENS=<値> OCR_OUTPUT_USD_PER_MILLION_TOKENS=<値>
```

| 環境変数 | 意味 |
|---|---|
| `OCR_FREE_BUDGET_YEN` | 1人ごとの累計の無料枠 (円、整数)。負値は無制限。ユーザーごとの上限 (管理画面) があればそちらを使う |
| `OCR_TOPUP_GRANT_YEN` | 買い足し1回で付ける量 (円、1以上の整数) |
| `OCR_INPUT_USD_PER_MILLION_TOKENS` | 入力 100 万トークンあたりのドル |
| `OCR_OUTPUT_USD_PER_MILLION_TOKENS` | 出力 (思考を含む) 100 万トークンあたりのドル |
| `OCR_JPY_PER_USD` | 1ドルの円換算 |

- 単価は Google の価格に合わせる。合わせないと、1人ごとの累計の金額が実際とずれて数えられる。
- 手元の開発では、`config.toml` の `[ocr] free_budget_yen`・`topup_grant_yen`・`[ocr.pricing]` でも与えられる (→ config.example.toml)。環境変数があればそちらを使う。

#### ディスクの空きを作る

```sh
just cloud-image-prune
docker image ls
docker image rm <要らない版>
```

- `just cloud-image-prune` は Docker のビルドキャッシュを全部消す (他のプロジェクトの分も)。次のビルドは最初から作り直しになる。

#### 困ったときに見るところ

| 症状 | 見るところ |
|---|---|
| 記録の登録・削除だけが 403 になる | プロキシが `Host` を書き換えているか、`trusted_proxies` の設定漏れ。ログに `CSRF: 同一オリジンチェック不一致` の warn が出る (突き合わせた host・scheme も一緒に出る) |
| ログインしても弾かれる・すぐログアウトされる | `secure_cookie = true` のまま http でアクセスしている |
| ログイン画面に Google のボタンが出ない | `GOOGLE_OAUTH_CLIENT_ID` / `_SECRET` が未設定 (起動ログに理由が出る) |
| ログイン画面に LINE のボタンが出ない | `LINE_LOGIN_CHANNEL_ID` / `_SECRET` が未設定 (起動ログに理由が出る) |
| 写真から読み取れない | `GEMINI_API_KEY` が未設定 (起動ログに理由が出る) |
| 起動直後に「写真の読み取り (GEMINI_API_KEY) が有効ですが、… がありません」などで終了する | シークレットに `OCR_*` が無い (→「写真の読み取りの無料枠・単価を変える」) |
| 「既に起動しています」で終了する | 同じデータディレクトリで二重に起動している |
| 起動直後に「設定ファイルに … がありません」で終了する | 必須の設定 (`[server] public_url`・`contact_url`、`[mail] host`・`from`) が `config.*.toml` に無い |
| 起動直後に「設定ファイルの解析に失敗しました」で終了する | `config.*.toml` の書き間違い。`fly logs` に行番号が出る |
| 起動直後に「CLOUD_ENV」のエラーで終了する | `fly.toml` の `[env]` に `CLOUD_ENV` が無い |
| 起動直後に「環境変数 SMTP_PASSWORD も設定してください」で終了する | そのアプリ (本番・検証用とも) のシークレットに `SMTP_PASSWORD` (Resend の API キー) が無い |
| 起動を繰り返して落ち続け、ログに Litestream の復元のエラーが出る | Volume を失った後の起動で、複製から戻せていない。シークレットの `LITESTREAM_*`・`R2_ACCOUNT_ID`、R2 の障害 |
| ログに Litestream の R2 への送信のエラーが出る | シークレットの `LITESTREAM_*`・`R2_ACCOUNT_ID`、`fly.toml` の `R2_BUCKET`、トークンがそのバケットに絞られているか |
| ログインの試行がすぐ上限に達する | 接続元を取り違えている (→ 初回セットアップ 6.「接続元を確かめる」) |
| 記録の登録などが 403 になる | `config.*.toml` の `proxy_headers = "fly"`。Cloudflare のプロキシを挟んでいないか |
| サインアップの確認メールが届かない | Resend でドメインの確認が済んでいるか (DKIM の TXT・`send.bp`・`rsend.bp` の CNAME。→ 初回セットアップ 4.)、`fly logs` に送信のエラーが出ていないか |
| `fly deploy` が Machine を2台作ろうとする | `--ha=false` を付けたか (`just deploy` は付けている) |

## 設定とデータの置き場所

設定ファイル (`config.toml`)・DB・セッション鍵・ログ・シングルインスタンスロックは以下の場所に置かれる。

- 環境変数 `BP_CARNET_HOME` が設定されていれば、すべてそのディレクトリ直下
  (Docker / systemd のように HOME が無い、あるいは配置場所を固定したい運用向け)
- それ以外は OS 標準のアプリデータディレクトリ
  - macOS: `~/Library/Application Support/com.amiiby.bp-carnet/`
  - Linux: 設定は `~/.config/bp-carnet/`、データは `~/.local/share/bp-carnet/`
  - Windows: 設定は `%APPDATA%\amiiby\bp-carnet\config\`、データは `%LOCALAPPDATA%\amiiby\bp-carnet\data\`

DB は `bp-carnet.db`。これが無く改名前の `bp-tracker.db` があれば、そちらを使う (本番の DB はこの名前のまま、→「デプロイ」)。

`config.toml` が無いと起動しない。`[server]` `public_url`・`contact_url` と `[mail]` `host`・`from` も必須。
開発では `config.example.toml` をそのまま写せば動く (メールは `just dev-mail` で受ける)。それ以外の項目は、書かなければ `config.example.toml` に示す既定値になる。

- `[server]` `bind` / `port` (既定: `0.0.0.0:3000`。dev では `frontend/vite.config.ts` の
  `/api` プロキシ先もこの `port` を直接読む)
- `[log]` `filter` (tracing EnvFilter 書式、`RUST_LOG` があれば優先) / `output` (`stdout` | `file`)
- `[session]` `secret` (空なら自動生成して `session.key` に永続化) / `secure_cookie` / `expiry_days`
- `[password]` `min_length` / `required_classes` (パスワードの最小強度。既定は4桁の数字PINを通す緩さ)
- `[server]` `public_url` (Google・LINE ログインのリダイレクトURIとメール内リンクの組み立てに使用。Client ID/Secret は
  `GOOGLE_OAUTH_CLIENT_ID` / `_SECRET`・`LINE_LOGIN_CHANNEL_ID` / `_SECRET` を環境変数 (`.env` 可) で渡す)
- `[server]` `contact_url` (アカウントの衝突・凍結などで案内する問い合わせ先)
- `[mail]` `host` / `port` / `tls` / `username` / `from` (SMTP。パスワードは環境変数 `SMTP_PASSWORD`)

リポジトリ直下に `.env` があれば起動時に `dotenvy` で自動読み込みする。用途は
`.env.example` を参照 (API キー等、config.toml に書くべきでない値)。

## CLI

```
bp-carnet                                サーバーを起動する
bp-carnet --create-user <name> [--admin] ユーザーを作成する (パスワードは対話入力、--admin で管理者)
bp-carnet --reset-password <name>        パスワードを再設定する (パスワードは対話入力)
bp-carnet --openapi                      OpenAPI 仕様 (JSON) を標準出力に書き出す
bp-carnet -v | --version                 バージョンを表示する
```

## API

`/api/v1` 配下。仕様は `openapi.json` (コミット対象) を参照。API を変更したら
`just api-types` で `openapi.json` と `frontend/src/lib/api/schema.d.ts` を再生成してコミットする。

エラーは常に `{"error":{"code":"...","message":"..."}}` の形で返る。frontend は `code` だけを見て
表示文言を決める (`frontend/src/lib/api/errors.ts`)。

## その他のコマンド

```sh
just openapi     # openapi.json を生成
just api-types   # openapi.json から frontend 用の TypeScript 型を生成
just api-types-check # 上記2ファイルの再生成漏れを検査 (書き換えない)
just sqlx-prepare # sqlx::query! 系マクロのオフラインキャッシュ (.sqlx/) を再生成
just licenses    # iPhone アプリの部品のライセンス表示を作り直す (要ネット。→ docs/third-party-licenses.md)
just licenses-check # ライセンス表示が今の依存と合っているかと、サーバーの依存に AGPL などが無いかを検査 (書き換えない)
just ios-packages-check # iPhone アプリの Swift のパッケージが Capacitor のプラグインと合っているかを検査 (cap update ios で Package.swift は書き換える)
just spec <dir>  # 画面ごとの仕様書に実写を差し込んで生成し、mo に追加する (要: mo。原稿はリポジトリの外)
just fmt         # コード整形 (cargo fmt + prettier)
just lint        # Lint (clippy + eslint/prettier check)
just check       # 型検査 (cargo check + svelte-check)
just ci          # fmt-check → lint → check → api-types-check → licenses-check → ios-packages-check → test → build → e2e-local を一括実行
just clean       # ビルド成果物を削除
```

コマンド一覧は `just --list` でも確認できる。
