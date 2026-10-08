#!/bin/sh
# 本番用コンテナの入口 (DEVELOPMENT.md「デプロイ」)。root で起動する。
#
# ADR: Fly.io の Volume は root の持ち物として /data に付くので、所有者を直してから UID 10001 に切り替える。
# litestream はアプリと同じ UID で動かす。DB は 0600 で、litestream は DB の隣にメタデータを書くため。
set -eu

: "${CLOUD_ENV:?fly.toml の [env] に CLOUD_ENV (production / staging) を書く}"

# 中身も直す。手順の外で root のまま DB などを作ってしまっても、アプリが読み書きできるように。
chown -R 10001:10001 /data
chmod 700 /data
# 設定はイメージに入れた版を使う。Volume に残った古い設定で起動しないよう、毎回写す。
install -o 10001 -g 10001 -m 600 "/etc/bp-carnet/config.${CLOUD_ENV}.toml" /data/config.toml

# メンテナンス中は、アプリも複製も起動せずに待つ。DB を差し替えるため (DEVELOPMENT.md「ある時点へ戻す」)。
# `fly secrets set MAINTENANCE=1` で入り、`fly secrets unset MAINTENANCE` で戻る (どちらも Machine が起動し直す)。
if [ "${MAINTENANCE:-}" = 1 ]; then
	echo "MAINTENANCE=1 のため、アプリを起動せずに待ちます"
	exec sleep infinity
fi

# litestream が bp-carnet を子プロセスとして起動し、終了のシグナルも渡す (→ litestream.yml)。
exec setpriv --reuid=10001 --regid=10001 --clear-groups litestream replicate -config /etc/litestream.yml
