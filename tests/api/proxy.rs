//! プロキシの後ろで動くときのヘッダーの扱い (docs/deployment.md)。

use super::*;

/// リバースプロキシ配下 (`[server] trusted_proxies` に接続元を登録した状態) のアプリ。
fn config_behind_proxy() -> Config {
    let mut config = test_config();
    config.server.trusted_proxies = vec!["127.0.0.1".parse().expect("固定値なので解析できる")];
    config
}

async fn test_app_behind_proxy(pool: SqlitePool) -> Router {
    test_app_with_config(pool, &config_behind_proxy()).await
}

/// Fly.io に置くとき (`proxy_headers = "fly"`) の、fly-proxy からのリクエスト。
fn fly_login_request(extra: &[(header::HeaderName, &str)]) -> Request<Body> {
    let mut headers = vec![
        (header::HOST, "bp.example.com"),
        (
            header::HeaderName::from_static("fly-forwarded-proto"),
            "https",
        ),
        (
            header::HeaderName::from_static("fly-client-ip"),
            "203.0.113.7",
        ),
    ];
    headers.extend(extra.iter().cloned());
    from_peer(login_request_with_headers(&headers), "172.16.5.10:54321")
}

async fn test_app_on_fly(pool: SqlitePool) -> Router {
    let mut config = test_config();
    config.server.trusted_proxies = vec!["172.16.0.0/12".parse().expect("固定値なので解析できる")];
    config.server.proxy_headers = bp_carnet::config::ProxyHeaders::Fly;
    test_app_with_config(pool, &config).await
}

/// 設定したプロキシの種類・接続元に合わせて、転送のヘッダー (`bp_carnet::forwarded`) を CSRF チェックが
/// 使うことを確かめる。どのヘッダーを読むかの場合ごとの確かめは `src/forwarded.rs` のテスト。ここでは、
/// 設定と接続元のアドレスが判定に渡っていることを、通る場合と拒む場合の両方で確かめる。
#[sqlx::test]
async fn csrf_check_reads_forwarded_headers_from_the_configured_proxy(pool: SqlitePool) {
    insert_user(&pool, "admin", "password").await;
    let behind_proxy = test_app_behind_proxy(pool.clone()).await;
    let on_fly = test_app_on_fly(pool).await;
    let x_forwarded_host = header::HeaderName::from_static("x-forwarded-host");
    let x_forwarded_proto = header::HeaderName::from_static("x-forwarded-proto");

    let cases = [
        (
            "https origin via X-Forwarded-* from a trusted proxy",
            &behind_proxy,
            from_peer(
                login_request_with_headers(&[
                    (header::HOST, "127.0.0.1:3000"),
                    (x_forwarded_host.clone(), "bp.example.com"),
                    (x_forwarded_proto.clone(), "https"),
                    (header::ORIGIN, "https://bp.example.com"),
                ]),
                "127.0.0.1:54321",
            ),
            StatusCode::OK,
        ),
        // `X-Forwarded-Proto` が得られる構成では scheme も比べる。
        (
            "http origin when the proxy terminates https",
            &behind_proxy,
            from_peer(
                login_request_with_headers(&[
                    (header::HOST, "bp.example.com"),
                    (x_forwarded_proto.clone(), "https"),
                    (header::ORIGIN, "http://bp.example.com"),
                ]),
                "127.0.0.1:54321",
            ),
            StatusCode::FORBIDDEN,
        ),
        (
            "X-Forwarded-Host from an untrusted peer",
            &behind_proxy,
            from_peer(
                login_request_with_headers(&[
                    (header::HOST, "bp.example.com"),
                    (x_forwarded_host.clone(), "evil.example"),
                    (header::ORIGIN, "https://evil.example"),
                ]),
                "192.168.1.9:54321",
            ),
            StatusCode::FORBIDDEN,
        ),
        (
            "https origin on Fly.io",
            &on_fly,
            fly_login_request(&[(header::ORIGIN, "https://bp.example.com")]),
            StatusCode::OK,
        ),
        // fly-proxy はクライアントが送った `X-Forwarded-Host` をそのまま渡すので、Fly.io では読まない。
        (
            "client-supplied X-Forwarded-Host on Fly.io",
            &on_fly,
            fly_login_request(&[
                (x_forwarded_host.clone(), "evil.example"),
                (header::ORIGIN, "https://evil.example"),
            ]),
            StatusCode::FORBIDDEN,
        ),
    ];

    for (name, app, request, expected) in cases {
        let response = send(app, request).await;
        if expected == StatusCode::FORBIDDEN {
            assert_error_case(response, expected, "csrf_rejected", name).await;
        } else {
            assert_eq!(response.status(), expected, "{name}");
        }
    }
}
