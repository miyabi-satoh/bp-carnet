# 本番用のイメージ (DEVELOPMENT.md「デプロイ」)。frontend と Rust のビルドもこの中で行う。
# 本番と検証用の app で同じイメージを使い、設定は CLOUD_ENV (fly.toml の [env]) で選ぶ。
#
# ADR: ビルドは amd64 の Windows でも Apple Silicon の Mac でもよい (出来るイメージは同じ)。Fly.io の
# Machine は amd64 だけで、Apple Silicon の Mac では amd64 をエミュレーションしながら rustc を動かすため初回が遅い。
# 効率のよいほうを使う。
#
# ビルドコンテキストはリポジトリのルート。除外するものは Dockerfile.dockerignore。
# pnpm のストアと cargo のレジストリ・target はキャッシュマウントに置き、イメージには入れない
# (2回目以降を速くするため。消すときは `just cloud-image-prune`)。

FROM node:24-bookworm-slim AS frontend
# frontend/package.json の packageManager と揃える。
RUN npm install -g pnpm@11.3.0
WORKDIR /app/frontend
# アプリのプラグインは package.json から link: で指すので、中身は使わないが install のために置く。
COPY mobile/plugins/ ../mobile/plugins/
COPY frontend/ ./
RUN --mount=type=cache,target=/pnpm-store \
    pnpm install --frozen-lockfile --store-dir /pnpm-store \
    && pnpm run build

# 実行用の debian:12-slim と glibc の版を揃えるため、同じ Debian 12 (bookworm) の Rust でビルドする。
FROM rust:1.95-bookworm AS backend
WORKDIR /app
COPY Cargo.toml Cargo.lock ./
COPY .sqlx/ .sqlx/
COPY migrations/ migrations/
COPY src/ src/
# rust_embed がリリースビルドに埋め込む。
COPY --from=frontend /app/frontend/build/ frontend/build/
# sqlx::query! マクロはコミット済みの .sqlx/ で検査する (DB に接続しない)。
ENV SQLX_OFFLINE=true
# target はキャッシュマウントでイメージの層に残らないため、バイナリは外へ写してから次の段へ渡す。
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/usr/local/cargo/git \
    --mount=type=cache,target=/app/target \
    cargo build --release --locked \
    && install -D target/release/bp-carnet /out/bp-carnet

FROM litestream/litestream:0.5.17 AS litestream

FROM debian:12-slim

# sqlite3 は Machine の中で DB を調べるため (→ DEVELOPMENT.md「DB の中を見る」)。アプリは使わない。
RUN apt-get update \
    && apt-get install -y --no-install-recommends sqlite3 \
    && rm -rf /var/lib/apt/lists/*

COPY --from=backend --chmod=755 /out/bp-carnet /usr/local/bin/bp-carnet
# 静的リンクのバイナリ (CA 証明書も内蔵)。
COPY --from=litestream /usr/local/bin/litestream /usr/local/bin/litestream
COPY --chmod=755 deploy/cloud/start.sh /usr/local/bin/start.sh
COPY deploy/cloud/litestream.yml /etc/litestream.yml
COPY deploy/cloud/config.production.toml deploy/cloud/config.staging.toml /etc/bp-carnet/

ENV BP_CARNET_HOME=/data
EXPOSE 3000
# root で起動し、Volume の所有者を直してから UID 10001 に切り替える (→ start.sh)。
ENTRYPOINT ["start.sh"]
