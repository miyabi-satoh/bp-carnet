//! OCR。Gemini は呼ばず、呼ぶ前に弾かれるものと同意・枠だけを確かめる。

use super::*;

/// 写真の欄の (欄の名前, MIME タイプ, 中身)。
type ImageField<'a> = (&'a str, &'a str, &'a [u8]);

/// `multipart/form-data` のリクエストを組み立てる。`field` が `None` ならフィールドを
/// 一つも含まない (空の) ボディにする。
fn multipart_image_request(
    uri: &str,
    cookie: Option<&str>,
    field: Option<ImageField>,
) -> Request<Body> {
    const BOUNDARY: &str = "bp-carnet-test-boundary";
    let mut body = Vec::new();
    if let Some((name, mime_type, bytes)) = field {
        body.extend_from_slice(format!("--{BOUNDARY}\r\n").as_bytes());
        body.extend_from_slice(
            format!("Content-Disposition: form-data; name=\"{name}\"; filename=\"photo\"\r\n")
                .as_bytes(),
        );
        body.extend_from_slice(format!("Content-Type: {mime_type}\r\n\r\n").as_bytes());
        body.extend_from_slice(bytes);
        body.extend_from_slice(b"\r\n");
    }
    body.extend_from_slice(format!("--{BOUNDARY}--\r\n").as_bytes());

    let mut builder = Request::post(uri).header(
        header::CONTENT_TYPE,
        format!("multipart/form-data; boundary={BOUNDARY}"),
    );
    if let Some(cookie) = cookie {
        builder = builder.header(header::COOKIE, cookie);
    }
    builder
        .body(Body::from(body))
        .expect("failed to build multipart request")
}

#[sqlx::test]
async fn ocr_status_reports_disabled_when_api_key_missing(pool: SqlitePool) {
    let (app, cookie) = logged_in_app(pool).await;

    let response = send(&app, get("/api/v1/ocr/status", Some(&cookie))).await;

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(json_body(response).await["enabled"], false);
}

#[sqlx::test]
async fn ocr_status_reports_enabled_when_configured(pool: SqlitePool) {
    insert_user(&pool, "admin", "password").await;
    let app = test_app_with_ocr_enabled(pool).await;
    let cookie = login_and_get_cookie(&app, "admin", "password").await;

    let response = send(&app, get("/api/v1/ocr/status", Some(&cookie))).await;

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(json_body(response).await["enabled"], true);
}

/// 写真を Gemini に送ることに同意した状態にする (全ユーザー)。
async fn consent_to_ocr(pool: &SqlitePool) {
    sqlx::query!("UPDATE users SET ocr_consented_at = '2026-09-26T00:00:00.000Z'")
        .execute(pool)
        .await
        .expect("failed to set ocr consent");
}

/// 同意していなければ、写真を受け取る前に 403 で弾く。
#[sqlx::test]
async fn ocr_extract_requires_consent(pool: SqlitePool) {
    insert_user(&pool, "admin", "password").await;
    let app = test_app_with_ocr_enabled(pool.clone()).await;
    let cookie = login_and_get_cookie(&app, "admin", "password").await;

    let response = send(
        &app,
        multipart_image_request(
            "/api/v1/ocr",
            Some(&cookie),
            Some(("image", "image/jpeg", b"dummy-image-bytes")),
        ),
    )
    .await;

    assert_error(response, StatusCode::FORBIDDEN, "ocr_consent_required").await;
}

/// 同意と取り消しが `/ocr/status` の `consented` に反映される。
#[sqlx::test]
async fn ocr_consent_can_be_given_and_withdrawn(pool: SqlitePool) {
    insert_user(&pool, "admin", "password").await;
    let app = test_app_with_ocr_enabled(pool).await;
    let cookie = login_and_get_cookie(&app, "admin", "password").await;

    let consented = |app: Router, cookie: String| async move {
        let response = send(&app, get("/api/v1/ocr/status", Some(&cookie))).await;
        json_body(response).await["consented"].clone()
    };
    assert_eq!(consented(app.clone(), cookie.clone()).await, false);

    let response = send(
        &app,
        request(
            Method::POST,
            "/api/v1/ocr/consent",
            Some(&cookie),
            &[],
            None,
        ),
    )
    .await;
    assert_eq!(response.status(), StatusCode::NO_CONTENT);
    assert_eq!(consented(app.clone(), cookie.clone()).await, true);

    let response = send(&app, delete("/api/v1/ocr/consent", Some(&cookie))).await;
    assert_eq!(response.status(), StatusCode::NO_CONTENT);
    assert_eq!(consented(app, cookie).await, false);
}

/// `AuthUser` (`FromRequestParts`) がボディを消費する `AppMultipart` (`FromRequest`) より
/// 先に評価されることを確認する。先に評価されていなければ、この巨大なボディの読み取り中に
/// (認証エラーより先に) 413 等の別のエラーになってしまうはず。
#[sqlx::test]
async fn ocr_extract_checks_login_before_reading_the_body(pool: SqlitePool) {
    let app = test_app_with_ocr_enabled(pool).await;

    let oversized = vec![0u8; 10 * 1024 * 1024 + 1];
    let response = send(
        &app,
        multipart_image_request(
            "/api/v1/ocr",
            None,
            Some(("image", "image/jpeg", &oversized)),
        ),
    )
    .await;

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[sqlx::test]
async fn ocr_extract_returns_503_when_disabled(pool: SqlitePool) {
    let (app, cookie) = logged_in_app(pool).await;

    let response = send(
        &app,
        multipart_image_request(
            "/api/v1/ocr",
            Some(&cookie),
            Some(("image", "image/jpeg", b"not a real jpeg")),
        ),
    )
    .await;

    assert_error(response, StatusCode::SERVICE_UNAVAILABLE, "ocr_disabled").await;
}

/// 同意済みのユーザーでも、Gemini を呼ぶ前に写真の欄・大きさ・形式で弾く。
#[sqlx::test]
async fn ocr_extract_rejects_unusable_images_before_calling_gemini(pool: SqlitePool) {
    insert_user(&pool, "admin", "password").await;
    consent_to_ocr(&pool).await;
    let app = test_app_with_ocr_enabled(pool).await;
    let cookie = login_and_get_cookie(&app, "admin", "password").await;

    let oversized = vec![0u8; 10 * 1024 * 1024 + 1];
    let cases: [(&str, Option<ImageField>, StatusCode, &str); 3] = [
        (
            "no image field",
            None,
            StatusCode::BAD_REQUEST,
            "invalid_request_body",
        ),
        (
            "oversized image",
            Some(("image", "image/jpeg", &oversized)),
            StatusCode::PAYLOAD_TOO_LARGE,
            "ocr_image_too_large",
        ),
        (
            "unsupported mime type",
            Some(("image", "text/plain", b"hello")),
            StatusCode::UNPROCESSABLE_ENTITY,
            "ocr_unsupported_mime_type",
        ),
    ];
    for (case, field, status, code) in cases {
        let response = send(
            &app,
            multipart_image_request("/api/v1/ocr", Some(&cookie), field),
        )
        .await;
        assert_error_case(response, status, code, case).await;
    }
}

/// 累計金額の上限 (無料枠) を使い切ったユーザーは、Gemini を呼ぶ前に 429 で弾かれる。
#[sqlx::test]
async fn ocr_extract_rejects_when_the_free_budget_is_used_up(pool: SqlitePool) {
    insert_user(&pool, "admin", "password").await;
    consent_to_ocr(&pool).await;
    set_ocr_budget(&pool, 0, 0).await;
    let app = test_app_with_ocr_enabled(pool.clone()).await;
    let cookie = login_and_get_cookie(&app, "admin", "password").await;

    let response = send(
        &app,
        multipart_image_request(
            "/api/v1/ocr",
            Some(&cookie),
            Some(("image", "image/jpeg", b"dummy-image-bytes")),
        ),
    )
    .await;

    assert_error(
        response,
        StatusCode::TOO_MANY_REQUESTS,
        "ocr_budget_exhausted",
    )
    .await;
}
