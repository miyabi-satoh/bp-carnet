set windows-shell := ["cmd.exe", "/c"]

frontend_dir := "frontend"
# git 管理外の data/ (開発用の DB・設定・テスト画像)。git worktree の中からでも元の clone のものを使う。
# worktree ごとに写すと、DB とポートの設定が分かれるため (frontend/scripts/repo-paths.ts と同じ)。
# git の無いフォルダ (zip で落とした場合) では、git を呼ばずにこのフォルダの data/ にする。代入は使わない
# recipe でも評価されるので、git が失敗するとどのコマンドも動かなくなるため。
data_dir := if path_exists(justfile_directory() / ".git") == "true" { join(parent_directory(`git rev-parse --path-format=absolute --git-common-dir`), "data") } else { justfile_directory() / "data" }
# sqlx-cli に渡す開発用の DB。`sqlite://C:/...` は URL として読むとドライブのコロンが落ちるので、`//` を付けない。
dev_database_url := "sqlite:" + data_dir / "dev-runtime" / "bp-carnet.db"

[private]
default:
    @just --list --unsorted

[doc('frontend の依存と、E2E が使う Chromium を入れる')]
[group('セットアップ')]
install:
    cd {{ frontend_dir }} && pnpm install
    cd {{ frontend_dir }} && pnpm exec playwright install chromium

[doc('push 前に整形を確かめるフック (lefthook) を入れる')]
[group('セットアップ')]
hooks-install:
    lefthook install

[doc('frontend をビルドする (frontend/build/)')]
[group('ビルド')]
frontend-build:
    cd {{ frontend_dir }} && pnpm run build

[doc('frontend をビルドして iOS のアプリへ写す (本番につなぐ)')]
[group('アプリ')]
[macos]
app-sync:
    cd {{ frontend_dir }} && pnpm run build && pnpm exec cap sync ios

# app-sync の、シミュレータから開発用のサーバー (http://127.0.0.1:3010) につなぐ版。
# Google・LINE のログインは、.env の開発用のクライアント・チャネルを使う (無ければ止める。本番のものに黙って替わらないように)
[doc('frontend をビルドして iOS のアプリへ写す (開発用のサーバーにつなぐ)')]
[group('アプリ')]
[macos]
app-sync-dev:
    #!/usr/bin/env bash
    set -euo pipefail
    google=$(sed -n 's/^GOOGLE_OAUTH_CLIENT_ID=//p' .env)
    line=$(sed -n 's/^LINE_LOGIN_CHANNEL_ID=//p' .env)
    if [ -z "$google" ] || [ -z "$line" ]; then
      echo ".env に GOOGLE_OAUTH_CLIENT_ID と LINE_LOGIN_CHANNEL_ID が要ります" >&2
      exit 1
    fi
    cd {{ frontend_dir }}
    VITE_APP_API_ORIGIN=http://127.0.0.1:3010 VITE_APP_GOOGLE_SERVER_CLIENT_ID="$google" \
      VITE_APP_LINE_CHANNEL_ID="$line" pnpm run build
    pnpm exec cap sync ios

[doc('frontend と release のバイナリをビルドする')]
[group('ビルド')]
build: frontend-build
    cargo build --release

# 本番用のイメージを作る (frontend と Rust のビルドも Docker の中で行う)。タグは latest とコミット (戻すとき用)。→ DEVELOPMENT.md「デプロイ」
[doc('本番用の Docker イメージを作る')]
[group('デプロイ')]
cloud-image:
    docker build --platform linux/amd64 -f deploy/cloud/Dockerfile -t bp-carnet:latest -t "bp-carnet:{{ `git describe --always --dirty` }}" .

# Docker のビルドキャッシュを消す。他のプロジェクトのキャッシュも消える (→ DEVELOPMENT.md「ディスクの空きを作る」)
[doc('Docker のビルドキャッシュを消す (ほかのプロジェクトの分も)')]
[group('デプロイ')]
cloud-image-prune:
    docker builder prune -f

# 本番の稼働・利用状況を表示する (→ DEVELOPMENT.md「稼働・利用状況を見る」)。
# 利用状況は BP_REPORT_USER / BP_REPORT_PASSWORD (管理者) が要る。Machine を起こさないなら `just report --no-wake`
[doc('本番の稼働・利用状況を出す')]
[group('デプロイ')]
report *args:
    node scripts/ops-report.mjs {{ args }}

# 検証用はアクセスでは起きない (deploy/cloud/fly.staging.toml)。しばらく通信が無ければ止まる
[doc('検証用の app を起こす')]
[group('デプロイ')]
staging-start:
    fly machine start {{ `fly machine list --app bp-carnet-staging --quiet | tr -d '[:space:]'` }} --app bp-carnet-staging

[doc('検証用の app に載せる')]
[group('デプロイ')]
deploy-staging: (fly-deploy "deploy/cloud/fly.staging.toml" "bp-carnet-staging")

# 本番に載せる。先に deploy-staging で確かめる (→ DEVELOPMENT.md「デプロイ」)
[doc('本番に載せる (先に deploy-staging で確かめる)')]
[group('デプロイ')]
deploy: (fly-deploy "deploy/cloud/fly.toml" "bp-carnet")

# イメージを作って registry.fly.io へ送り、`fly deploy --image` で載せる (内部用)
# ADR: タグはコミット (`git describe`)。前の版に戻すときに、このタグを `fly deploy --image` に渡すため。
# `--ha=false` は、Volume が1つなので予備の Machine を作らせないため (既定は2台作る)。
[private]
fly-deploy config app: cloud-image
    docker tag bp-carnet:latest "registry.fly.io/{{ app }}:{{ `git describe --always --dirty` }}"
    fly auth docker
    docker push "registry.fly.io/{{ app }}:{{ `git describe --always --dirty` }}"
    fly deploy --ha=false --config {{ config }} --image "registry.fly.io/{{ app }}:{{ `git describe --always --dirty` }}"

# frontend/build が無ければビルドする (内部用)
[private]
ensure-frontend-build:
    @{{ if path_exists(justfile_directory() / frontend_dir / "build") == "true" { "echo frontend/build exists" } else { "just frontend-build" } }}

# backend を引数付きで起動する。無指定ならサーバー起動、--create-user/--openapi/-v等も渡せる(DBはdata/dev-runtime/配下)
[doc('backend を起動する (引数は本体に渡す。例: --create-user)')]
[group('開発')]
[unix]
dev-backend *args: ensure-frontend-build
    BP_CARNET_HOME="{{ data_dir }}/dev-runtime" cargo run -- {{ args }}

[doc('backend を起動する (引数は本体に渡す。例: --create-user)')]
[group('開発')]
[windows]
dev-backend *args: ensure-frontend-build
    set "BP_CARNET_HOME={{ data_dir }}\dev-runtime" && cargo run -- {{ args }}

[doc('frontend を開発モードで起動する (/api は backend へ)')]
[group('開発')]
dev-frontend:
    cd {{ frontend_dir }} && pnpm run dev

# 開発サーバーが送るメールを 127.0.0.1:1025 で受け取り、data/dev-runtime/mail/ に1通ずつ JSON で置く
# (ポートは config.example.toml の [mail] と合わせる)
[doc('開発サーバーが送るメールを data/dev-runtime/mail/ に受ける')]
[group('開発')]
dev-mail:
    cd {{ frontend_dir }} && node scripts/mail-sink.ts "{{ data_dir }}/dev-runtime/mail" 1025

[doc('backend・frontend・メールの受け口をまとめて起動する')]
[group('開発')]
dev:
    cd {{ frontend_dir }} && pnpm exec concurrently -n backend,frontend,mail -c blue,green,magenta "just dev-backend" "just dev-frontend" "just dev-mail"

# sqlx::query! 系マクロのオフラインキャッシュ(.sqlx/)を再生成する (クエリ変更時に実行してコミットする)。
# DB は just dev-backend と同じもの。.env の DATABASE_URL は相対パスで、worktree からは開けないため渡し直す
[doc('sqlx のオフラインキャッシュ (.sqlx/) を作り直す')]
[group('生成')]
sqlx-prepare: ensure-frontend-build
    cargo sqlx prepare --no-dotenv -D "{{ dev_database_url }}" -- --all-targets

[doc('開発用の DB を作り、マイグレーションを当てる')]
[group('開発')]
db-migrate:
    cargo sqlx database setup --no-dotenv -D "{{ dev_database_url }}"

# 画面ごとの仕様書に実写を差し込んで生成し、mo に追加する。ブラウザは開かない (要: mo)。
# 仕様書の原稿はこのリポジトリの外にあり、その置き場を `dir` で渡す
[doc('画面ごとの仕様書を作り、mo に出す (原稿の置き場を渡す)')]
[group('生成')]
[unix]
spec dir: frontend-build
    cargo build
    cd {{ frontend_dir }} && SPEC_SRC_DIR="{{ join(invocation_directory(), dir) }}" node scripts/generate-spec.ts
    mo -R docs/generated/spec/ --target bp-carnet-spec --no-open

# E2Eブラウザテスト (Playwright) を、起動済みの環境に対して実行する。backend は事前に起動しておくこと
# (`just dev-backend`・検証用の app 等)。管理者アカウントでログインするため、その
# ユーザーID・パスワードを渡す (ユーザーID省略時は"admin")。
# 例: just e2e http://127.0.0.1:3010 password
# 自己署名の証明書の環境は E2E_IGNORE_HTTPS_ERRORS=1 を付ける。
# 写真の読み取りのシナリオ (`@ocr`) は E2E_OCR=1 のときだけ流す。本物の Gemini を呼ぶので、向き先のサーバーに
# GEMINI_API_KEY が要る。テスト画像は git に入れていないため、流すマシンの `data/ocr-test-images/` に置く。
# E2E_KEEP_DATA=1 を付けると、テストが作ったユーザーと記録を片付けずに残し、終わりに ID・パスワードの一覧を出す
# (流した後に手でログインして確かめるため。残したユーザーは管理画面から削除する)。
# 引数は `$` 付きで環境変数として渡す。コマンド文字列に埋め込むと、Windows の cmd.exe が
# パスワード中の `%` や `"` を解釈して値や構文が壊れるため。
[doc('E2E を起動済みの環境に当てる (URL と管理者のパスワードを渡す)')]
[group('検査')]
e2e $E2E_BASE_URL $E2E_ADMIN_PASSWORD $E2E_ADMIN_USER="admin":
    cd {{ frontend_dir }} && pnpm run test:e2e

# E2Eブラウザテストを、使い捨てのbackend (一時BP_CARNET_HOME・空きポート・管理者1人) に対して実行する。
# 開発用DBを使わないので、途中で落ちても汚さない。`just ci` (CI) から呼ばれる
# メールは backend と一緒に立てる SMTP の受け口で受け取り、サインアップ・パスワードの再設定もリンクまで通す
# 古いフロントで通ったように見えないよう、frontend は毎回ビルドし直す (`just ci` では build と合わせて1回だけ走る)
[doc('E2E を使い捨ての backend に当てる')]
[group('検査')]
e2e-local: frontend-build
    cargo build
    cd {{ frontend_dir }} && node scripts/run-e2e.ts

# 今の画面を撮るため frontend は毎回ビルドし直す。書き出した画像をアプリに入れるには、もう一度 `just frontend-build` する。
# 確認を頼む前に、変えたページを headless で開き、画像が読めているか (`naturalWidth` が 0 でないか) を見る
# 使い方のページ (`/help`) の手順の画像を撮り直し、frontend/static/help/ に書き出す (使い捨ての backend で help.e2e.ts だけを流す)
[doc('使い方のページの手順の画像を撮り直す')]
[group('生成')]
help-shots: frontend-build
    cargo build
    cd {{ frontend_dir }} && node scripts/run-e2e.ts --help-shots

[doc('openapi.json を作り直す')]
[group('生成')]
openapi:
    cargo run --quiet -- --openapi > openapi.json

[doc('openapi.json と frontend の API の型を作り直す')]
[group('生成')]
api-types: openapi
    cd {{ frontend_dir }} && pnpm run generate:api-types

# openapi.json / schema.d.ts の再生成漏れを検査する (CI 向け、書き換えない)。
# target/debug のバイナリを作り直すため、dev-backend を常駐させたまま実行しないこと
[doc('openapi.json と API の型の作り直し漏れを見る')]
[group('検査')]
api-types-check:
    cd {{ frontend_dir }} && node scripts/check-api-types.ts

# 使っている部品のライセンス表示のうち、iPhone アプリの分を作り直す (要ネット。→ docs/third-party-licenses.md)。
# 画面の分はビルドのたびに作る。依存を変えたら、結果をコミットする
[doc('iPhone アプリの部品のライセンス表示を作り直す (要ネット)')]
[group('生成')]
licenses:
    cd {{ frontend_dir }} && node scripts/generate-ios-licenses.ts

[doc('ライセンス表示の作り直し漏れと、サーバーの依存のライセンスを見る')]
[group('検査')]
licenses-check:
    cd {{ frontend_dir }} && node scripts/generate-ios-licenses.ts --check && node scripts/check-server-licenses.ts

[doc('iPhone アプリの Swift のパッケージが、入れてある Capacitor のプラグインと食い違っていないかを見る')]
[group('検査')]
ios-packages-check: ensure-frontend-build
    cd {{ frontend_dir }} && pnpm exec cap update ios && node scripts/check-ios-packages.ts

[doc('仕様書に書き漏れた画面が無いかを見る (原稿の置き場を渡す)')]
[group('検査')]
[unix]
spec-coverage dir:
    cd {{ frontend_dir }} && SPEC_SRC_DIR="{{ join(invocation_directory(), dir) }}" node scripts/check-spec-coverage.ts

[doc('コードを整形する (cargo fmt + prettier)')]
[group('生成')]
fmt:
    cargo fmt
    cd {{ frontend_dir }} && pnpm run format

[doc('整形の崩れを見る (書き換えない)')]
[group('検査')]
fmt-check:
    cargo fmt --check
    cd {{ frontend_dir }} && pnpm exec prettier --check .

[doc('clippy・eslint・prettier を流す')]
[group('検査')]
lint: ensure-frontend-build
    cargo clippy --all-targets -- -D warnings
    cd {{ frontend_dir }} && pnpm run lint

[doc('型検査を流す (cargo check + svelte-check)')]
[group('検査')]
check: ensure-frontend-build
    cargo check
    cd {{ frontend_dir }} && pnpm run check

[doc('単体テストを流す (cargo test + vitest)')]
[group('検査')]
test: ensure-frontend-build
    cargo test
    cd {{ frontend_dir }} && pnpm run test:unit -- --run

# 一連の品質チェック (フォーマット→lint→型検査→生成物→test→build→e2e)。CI (.github/workflows/ci.yml) が PR ごとに流す。
# e2e は build の後に置き、その時点のフロントで流す
[doc('検査を全部流す (CI が PR ごとに流す)')]
[group('検査')]
ci: fmt-check lint check api-types-check licenses-check ios-packages-check test build e2e-local

[doc('ビルドの成果物を消す')]
[group('ビルド')]
[unix]
clean:
    cargo clean
    rm -rf {{ frontend_dir }}/build {{ frontend_dir }}/.svelte-kit

[doc('ビルドの成果物を消す')]
[group('ビルド')]
[windows]
clean:
    cargo clean
    if exist {{ frontend_dir }}\build rmdir /s /q {{ frontend_dir }}\build
    if exist {{ frontend_dir }}\.svelte-kit rmdir /s /q {{ frontend_dir }}\.svelte-kit
