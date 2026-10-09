//! 公開ページ・ヘルスチェック・未知のパス・検索エンジン向けの出力。

use super::*;

#[sqlx::test]
async fn health_returns_ok_with_version(pool: SqlitePool) {
    let app = test_app(pool).await;

    let response = send(&app, get("/api/v1/health", None)).await;

    assert_eq!(response.status(), StatusCode::OK);
    let body = json_body(response).await;
    assert_eq!(body["status"], "ok");
    assert_eq!(body["version"], env!("CARGO_PKG_VERSION"));
}

#[sqlx::test]
async fn health_returns_503_when_db_unavailable(pool: SqlitePool) {
    let app = test_app(pool.clone()).await;
    pool.close().await;

    let response = send(&app, get("/api/v1/health", None)).await;

    assert_error(
        response,
        StatusCode::SERVICE_UNAVAILABLE,
        "database_unavailable",
    )
    .await;
}

#[sqlx::test]
async fn unknown_api_path_returns_json_404(pool: SqlitePool) {
    let app = test_app(pool).await;

    let response = send(&app, get("/api/v1/nope", None)).await;

    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    assert_eq!(
        response
            .headers()
            .get(header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok()),
        Some("application/json")
    );
    assert_eq!(json_body(response).await["error"]["code"], "not_found");
}

#[sqlx::test]
async fn unknown_non_api_path_falls_back_to_spa(pool: SqlitePool) {
    let app = test_app(pool).await;

    let response = send(&app, get("/nope", None)).await;

    // frontend/build に index.html がある前提 (無ければ 404)。
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response
            .headers()
            .get(header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok()),
        Some("text/html")
    );
    assert_eq!(
        response
            .headers()
            .get(header::CACHE_CONTROL)
            .and_then(|v| v.to_str().ok()),
        Some("no-cache")
    );
}

// 検索エンジン向けの出力 (docs/architecture.md)。ページごとのタグ・sitemap・robots.txt の中身は
// `src/seo.rs` のテスト。ここでは、配信の経路ごとに設定の公開 URL で差し込まれ、ヘッダーが付くことを確かめる。

const SEO_URL: &str = "https://bp.example.com";

async fn test_app_with_public_url(pool: SqlitePool) -> Router {
    let mut config = test_config();
    config.server.public_url = SEO_URL.to_string();
    test_app_with_config(pool, &config).await
}

/// (ステータス, `X-Robots-Tag`, 本文)
async fn fetch_text(app: &Router, uri: &str) -> (StatusCode, Option<String>, String) {
    let response = send(app, get(uri, None)).await;
    let status = response.status();
    let robots = response
        .headers()
        .get("x-robots-tag")
        .and_then(|v| v.to_str().ok())
        .map(str::to_string);
    let body = String::from_utf8(body_bytes(response).await).expect("本文は UTF-8");
    (status, robots, body)
}

/// トップ (`/`) と、SPA の振り分けで返すページの両方に、`<head>` の中へタグを差し込む。
#[sqlx::test]
async fn public_pages_carry_canonical_and_ogp(pool: SqlitePool) {
    let app = test_app_with_public_url(pool).await;

    for path in ["/", "/terms"] {
        let (status, robots, html) = fetch_text(&app, path).await;
        assert_eq!(status, StatusCode::OK, "{path}");
        assert_eq!(robots, None, "{path}");
        let canonical = if path == "/" {
            format!("{SEO_URL}/")
        } else {
            format!("{SEO_URL}{path}")
        };
        assert!(
            html.contains(&format!(r#"<link rel="canonical" href="{canonical}" />"#)),
            "{path}"
        );
        assert!(html.contains(r#"property="og:title""#), "{path}");
        assert!(!html.contains("noindex"), "{path}");
        assert!(
            html.find("canonical").expect("canonical") < html.find("</head>").expect("</head>"),
            "{path}"
        );
    }
}

/// 索引させないページは、SPA の振り分けで返すものも静的なファイルとして返すものも、
/// `X-Robots-Tag` と meta の両方で noindex にする。
#[sqlx::test]
async fn private_and_unknown_pages_are_noindex(pool: SqlitePool) {
    let app = test_app_with_public_url(pool).await;

    for path in ["/login", "/index.html"] {
        let (status, robots, html) = fetch_text(&app, path).await;
        assert_eq!(status, StatusCode::OK, "{path}");
        assert_eq!(robots.as_deref(), Some("noindex"), "{path}");
        assert!(
            html.contains(r#"<meta name="robots" content="noindex" />"#),
            "{path}"
        );
        assert!(!html.contains("canonical"), "{path}");
    }
}

#[sqlx::test]
async fn assets_do_not_get_noindex(pool: SqlitePool) {
    let app = test_app_with_public_url(pool).await;

    for path in ["/robots.txt", "/manifest.webmanifest"] {
        let response = send(&app, get(path, None)).await;
        assert_eq!(response.status(), StatusCode::OK, "{path}");
        assert!(response.headers().get("x-robots-tag").is_none(), "{path}");
    }
}

#[sqlx::test]
async fn sitemap_uses_the_public_url(pool: SqlitePool) {
    let app = test_app_with_public_url(pool).await;

    let response = send(&app, get("/sitemap.xml", None)).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response
            .headers()
            .get(header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok()),
        Some("application/xml")
    );
    let xml = String::from_utf8(body_bytes(response).await).expect("本文は UTF-8");
    assert!(xml.contains("<url><loc>https://bp.example.com/terms</loc></url>"));
}

#[sqlx::test]
async fn robots_txt_points_at_the_sitemap(pool: SqlitePool) {
    let app = test_app_with_public_url(pool).await;
    let (status, _, body) = fetch_text(&app, "/robots.txt").await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.contains("Sitemap: https://bp.example.com/sitemap.xml"));
}

#[sqlx::test]
async fn tokushoho_moves_permanently_to_the_shared_page(pool: SqlitePool) {
    let app = test_app_with_public_url(pool).await;

    for path in ["/tokushoho", "/tokushoho/"] {
        let response = send(&app, get(path, None)).await;
        assert_eq!(response.status(), StatusCode::MOVED_PERMANENTLY, "{path}");
        assert_eq!(
            response
                .headers()
                .get(header::LOCATION)
                .and_then(|v| v.to_str().ok()),
            Some("https://amiiby.com/tokushoho/"),
            "{path}"
        );
    }
}
