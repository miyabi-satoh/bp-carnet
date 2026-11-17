//! すべての応答に付けるセキュリティヘッダー (docs/architecture.md「セキュリティヘッダー」)。
//! CSP の中身は `src/security_headers.rs` の単体テストで確かめ、ここでは応答の種類ごとに付くことを確かめる。

use super::*;

fn header_value<'a>(response: &'a Response, name: &str) -> Option<&'a str> {
    response
        .headers()
        .get(name)
        .map(|value| value.to_str().expect("ASCII のはず"))
}

/// API の応答 (外側の層の断りを含む) には、何も読ませない CSP を付ける。
#[sqlx::test]
async fn api_responses_carry_the_api_policy(pool: SqlitePool) {
    let app = test_app(pool).await;
    let ok = send(&app, get("/api/v1/health", None)).await;
    // 別のオリジンからの POST は、CSRF の層が 403 で断る。
    let rejected = send(
        &app,
        login_request_with_headers(&[(header::ORIGIN, "https://evil.example")]),
    )
    .await;
    assert_eq!(rejected.status(), StatusCode::FORBIDDEN);

    for response in [&ok, &rejected] {
        assert_eq!(
            header_value(response, "content-security-policy"),
            Some("default-src 'none'; frame-ancestors 'none'")
        );
        assert_eq!(
            header_value(response, "x-content-type-options"),
            Some("nosniff")
        );
        assert_eq!(
            header_value(response, "referrer-policy"),
            Some("strict-origin-when-cross-origin")
        );
        assert_eq!(header_value(response, "strict-transport-security"), None);
    }
}

/// 画面には、index.html のインラインスクリプトのハッシュを入れた CSP を付ける。
#[sqlx::test]
async fn pages_carry_the_page_policy_with_script_hashes(pool: SqlitePool) {
    let app = test_app(pool).await;
    let response = send(&app, get("/login", None)).await;
    assert_eq!(response.status(), StatusCode::OK);
    let policy = header_value(&response, "content-security-policy")
        .expect("CSP があるはず")
        .to_string();
    assert!(
        policy.starts_with("default-src 'self'; script-src 'self' 'sha256-"),
        "{policy}"
    );
    assert!(policy.contains("frame-ancestors 'none'"), "{policy}");
}

/// https で公開する設定では HSTS も付ける。
#[sqlx::test]
async fn secure_deployments_send_hsts(pool: SqlitePool) {
    let mut config = test_config();
    config.session.secure_cookie = true;
    let app = test_app_with_config(pool, &config).await;
    let response = send(&app, get("/api/v1/health", None)).await;
    assert_eq!(
        header_value(&response, "strict-transport-security"),
        Some("max-age=63072000")
    );
}
