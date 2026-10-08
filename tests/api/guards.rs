//! 全部の API にかかる守り (ログイン・管理者の権限・無いユーザー・CSRF)。API の一覧を表にして回す。

use super::*;

/// ログインが要る API の一覧。`{id}` には存在しない ID を入れる (守りが先に効けば 404 にならない)。
const LOGIN_REQUIRED_APIS: &[(Method, &str)] = &[
    (Method::GET, "/api/v1/auth/me"),
    (Method::POST, "/api/v1/account/deletion"),
    (Method::POST, "/api/v1/account/email"),
    (Method::DELETE, "/api/v1/account/identities/google"),
    (Method::PUT, "/api/v1/account/password"),
    (Method::GET, "/api/v1/records"),
    (Method::POST, "/api/v1/records"),
    (Method::GET, "/api/v1/records/export"),
    (Method::POST, "/api/v1/records/import"),
    (Method::GET, "/api/v1/records/latest-date"),
    (Method::GET, "/api/v1/records/summary"),
    (Method::GET, "/api/v1/records/1"),
    (Method::PUT, "/api/v1/records/1"),
    (Method::DELETE, "/api/v1/records/1"),
    (Method::GET, "/api/v1/settings"),
    (Method::PUT, "/api/v1/settings"),
    (Method::PUT, "/api/v1/settings/detected-timezone"),
    (Method::GET, "/api/v1/settings/timezones"),
    (Method::GET, "/api/v1/ocr/status"),
    (Method::POST, "/api/v1/ocr"),
    (Method::POST, "/api/v1/ocr/consent"),
    (Method::DELETE, "/api/v1/ocr/consent"),
    (Method::POST, "/api/v1/payments/ocr-topup/checkout"),
    (
        Method::GET,
        "/api/v1/payments/ocr-topup/result?sessionId=cs_1",
    ),
    (Method::POST, "/api/v1/payments/apple/account-token"),
    (Method::POST, "/api/v1/payments/apple/transactions"),
    (Method::GET, "/api/v1/admin/users"),
    (Method::POST, "/api/v1/admin/users"),
    (Method::GET, "/api/v1/admin/users/1"),
    (Method::GET, "/api/v1/admin/users/1/records"),
    (Method::PUT, "/api/v1/admin/users/1/frozen"),
    (Method::PUT, "/api/v1/admin/users/1/ocr-limit"),
    (Method::PUT, "/api/v1/admin/users/1/password"),
    (Method::POST, "/api/v1/admin/users/1/deletion"),
    (Method::DELETE, "/api/v1/admin/users/1/deletion"),
];

/// 本文を付けずに送る。本文の検証より先にログインを確かめていなければ、401 でなく 415・422 などになる。
#[sqlx::test]
async fn login_required_apis_reject_requests_without_a_session(pool: SqlitePool) {
    let app = test_app_with_ocr_enabled(pool).await;

    for (method, uri) in LOGIN_REQUIRED_APIS {
        let response = send(&app, request(method.clone(), uri, None, &[], None)).await;
        let label = format!("{method} {uri}");
        assert_error_case(response, StatusCode::UNAUTHORIZED, "unauthorized", &label).await;
    }
}

/// 一般ユーザーには 403 を返す。未ログインの 401 と区別できるコードにする
/// (frontend は 401 をセッション切れとしてログイン画面へ送るため)。拒んだ操作は何も変えない。
#[sqlx::test]
async fn admin_apis_reject_non_admins(pool: SqlitePool) {
    insert_user(&pool, "kyoko", "password").await;
    insert_user(&pool, "other", "password").await;
    insert_user(&pool, "scheduled", "password").await;
    set_deletion_scheduled(&pool, "scheduled", "2100-01-01T00:00:00.000Z", "admin").await;
    let target_id = user_id_of(&pool, "other").await;
    let scheduled_id = user_id_of(&pool, "scheduled").await;
    let app = test_app(pool.clone()).await;
    let cookie = login_and_get_cookie(&app, "kyoko", "password").await;

    let cases = [
        get("/api/v1/admin/users", Some(&cookie)),
        get(&format!("/api/v1/admin/users/{target_id}"), Some(&cookie)),
        get(
            &format!("/api/v1/admin/users/{target_id}/records"),
            Some(&cookie),
        ),
        create_user_request(
            &cookie,
            json!({"username": "newbie", "password": "password"}),
        ),
        set_frozen_request(&cookie, target_id, true),
        put_json(
            &format!("/api/v1/admin/users/{target_id}/ocr-limit"),
            Some(&cookie),
            json!({ "ocrBudgetYen": 80 }),
        ),
        reset_password_request(&cookie, target_id, "new-password"),
        admin_delete_request(&cookie, target_id),
        admin_cancel_deletion_request(&cookie, scheduled_id),
    ];
    for request in cases {
        let label = format!("{} {}", request.method(), request.uri());
        let response = send(&app, request).await;
        assert_error_case(response, StatusCode::FORBIDDEN, "forbidden", &label).await;
    }

    assert!(audit_log_entries(&pool).await.is_empty());
    assert_eq!(count_users(&pool).await, 3);
    assert!(!is_frozen(&pool, "other").await);
    assert_eq!(ocr_budget_yen_of(&pool, "other").await, None);
    let login = send(&app, login_request("other", "password")).await;
    assert_eq!(login.status(), StatusCode::OK);
    assert_eq!(session_generation_of(&pool, "other").await, 0);
    assert_eq!(deletion_scheduled_at_of(&pool, "other").await, None);
    assert!(deletion_scheduled_at_of(&pool, "scheduled").await.is_some());
}

/// 対象のユーザーが無ければ 404 を返し、監査ログにも残さない。
#[sqlx::test]
async fn admin_apis_return_404_for_unknown_users(pool: SqlitePool) {
    insert_admin(&pool, "admin", "password").await;
    let app = test_app(pool.clone()).await;
    let cookie = login_and_get_cookie(&app, "admin", "password").await;

    let cases = [
        get("/api/v1/admin/users/9999", Some(&cookie)),
        get("/api/v1/admin/users/9999/records", Some(&cookie)),
        set_frozen_request(&cookie, 9999, true),
        put_json(
            "/api/v1/admin/users/9999/ocr-limit",
            Some(&cookie),
            json!({ "ocrBudgetYen": 3 }),
        ),
        reset_password_request(&cookie, 9999, "new-password"),
        admin_delete_request(&cookie, 9999),
        admin_cancel_deletion_request(&cookie, 9999),
    ];
    for request in cases {
        let label = format!("{} {}", request.method(), request.uri());
        let response = send(&app, request).await;
        assert_error_case(response, StatusCode::NOT_FOUND, "not_found", &label).await;
    }
    assert!(audit_log_entries(&pool).await.is_empty());
}

/// CSRF の同一オリジンチェック (`bp_carnet::csrf::same_origin_check`)。GET 以外で、`Origin` か
/// `Referer` が `Host` と食い違えば 403 にする。
#[sqlx::test]
async fn csrf_check_rejects_only_cross_origin_requests_other_than_get(pool: SqlitePool) {
    let (app, cookie) = logged_in_app(pool).await;
    let host = (header::HOST, "bp-carnet.example");
    let evil_origin = (header::ORIGIN, "https://evil.example");

    let cases = [
        (
            "login: Origin が違う",
            login_request_with_headers(&[host.clone(), evil_origin.clone()]),
            StatusCode::FORBIDDEN,
        ),
        (
            "login: Referer が違う",
            login_request_with_headers(&[
                host.clone(),
                (header::REFERER, "https://evil.example/login"),
            ]),
            StatusCode::FORBIDDEN,
        ),
        (
            "login: Origin が同じ",
            login_request_with_headers(&[
                host.clone(),
                (header::ORIGIN, "https://bp-carnet.example"),
            ]),
            StatusCode::OK,
        ),
        (
            "login: Referer が同じ",
            login_request_with_headers(&[
                host.clone(),
                (header::REFERER, "https://bp-carnet.example/login"),
            ]),
            StatusCode::OK,
        ),
        // サンドボックス化された iframe などの `Origin: null` は、authority が取れないので拒む。
        (
            "login: Origin が null",
            login_request_with_headers(&[host.clone(), (header::ORIGIN, "null")]),
            StatusCode::FORBIDDEN,
        ),
        // Origin・Referer を送らないクライアントは通す。ほかのテストの要求もこれらを付けていない。
        (
            "login: Origin・Referer が無い",
            login_request_with_headers(std::slice::from_ref(&host)),
            StatusCode::OK,
        ),
        // HTTP/2 などで `Host` が無ければ、URI の authority と比べる。
        (
            "login: Host が無く URI の authority と同じ",
            Request::post("http://bp-carnet.example/api/v1/auth/login")
                .header(header::CONTENT_TYPE, "application/json")
                .header(header::ORIGIN, "http://bp-carnet.example")
                .body(Body::from(
                    json!({"username": "admin", "password": "password"}).to_string(),
                ))
                .expect("failed to build request"),
            StatusCode::OK,
        ),
        (
            "PUT record: Origin が違う",
            request(
                Method::PUT,
                &record_uri(1),
                Some(&cookie),
                &[host.clone(), evil_origin.clone()],
                Some(valid_record_body()),
            ),
            StatusCode::FORBIDDEN,
        ),
        // CSRF は認証より外側のミドルウェアなので、未ログインでも 401 より先に 403 になる (`src/lib.rs` のレイヤー順)。
        (
            "PUT record: 未ログインで Origin が違う",
            request(
                Method::PUT,
                &record_uri(1),
                None,
                &[host.clone(), evil_origin.clone()],
                Some(valid_record_body()),
            ),
            StatusCode::FORBIDDEN,
        ),
        (
            "GET records: Origin が違っても通す",
            request(
                Method::GET,
                "/api/v1/records",
                Some(&cookie),
                &[host.clone(), evil_origin.clone()],
                None,
            ),
            StatusCode::OK,
        ),
        // HEAD も「GET 以外」として確かめる (`src/csrf.rs` のモジュールコメント)。
        (
            "HEAD records: Origin が違う",
            request(
                Method::HEAD,
                "/api/v1/records",
                None,
                &[host.clone(), evil_origin.clone()],
                None,
            ),
            StatusCode::FORBIDDEN,
        ),
    ];
    for (label, request, expected) in cases {
        let is_head = request.method() == Method::HEAD;
        let response = send(&app, request).await;
        // HEAD の応答は本文が空になるので、code は確かめず状態だけにする。
        if expected == StatusCode::FORBIDDEN && !is_head {
            assert_error_case(response, expected, "csrf_rejected", label).await;
        } else {
            assert_eq!(response.status(), expected, "{label}");
        }
    }
}
