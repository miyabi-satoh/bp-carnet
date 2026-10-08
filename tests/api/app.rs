//! モバイルアプリのトークン認証 (docs/mobile-app.md)。

use super::*;

async fn app_login(app: &Router, username: &str, password: &str) -> String {
    let response = send(
        app,
        app_request(
            Method::POST,
            "/api/v1/app/auth/login",
            None,
            Some(json!({"username": username, "password": password})),
        ),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    assert!(
        response.headers().get(header::SET_COOKIE).is_none(),
        "アプリのログインではセッションの Cookie を発行しない"
    );
    let body = json_body(response).await;
    assert_eq!(body["user"]["username"], username);
    body["token"]
        .as_str()
        .expect("token should be a string")
        .to_string()
}

#[sqlx::test]
async fn app_login_returns_a_token_that_authenticates(pool: SqlitePool) {
    insert_user(&pool, "admin", "password").await;
    let app = test_app(pool).await;

    let token = app_login(&app, "admin", "password").await;
    assert_eq!(app_me_status(&app, &token).await, StatusCode::OK);
    assert_eq!(
        app_me_status(&app, "not-a-token").await,
        StatusCode::UNAUTHORIZED
    );
}

#[sqlx::test]
async fn app_login_rejects_wrong_password_and_frozen_accounts(pool: SqlitePool) {
    insert_user(&pool, "admin", "password").await;
    insert_user(&pool, "frozen", "password").await;
    freeze_user(&pool, "frozen").await;
    let app = test_app(pool).await;

    for (username, password, expected) in [
        ("admin", "wrong", StatusCode::UNAUTHORIZED),
        ("frozen", "password", StatusCode::FORBIDDEN),
    ] {
        let response = send(
            &app,
            app_request(
                Method::POST,
                "/api/v1/app/auth/login",
                None,
                Some(json!({"username": username, "password": password})),
            ),
        )
        .await;
        assert_eq!(response.status(), expected, "{username}");
    }
}

#[sqlx::test]
async fn app_logout_revokes_the_token(pool: SqlitePool) {
    insert_user(&pool, "admin", "password").await;
    let app = test_app(pool).await;
    let token = app_login(&app, "admin", "password").await;

    let response = send(
        &app,
        app_request(Method::POST, "/api/v1/app/auth/logout", Some(&token), None),
    )
    .await;
    assert_eq!(response.status(), StatusCode::NO_CONTENT);
    assert_eq!(app_me_status(&app, &token).await, StatusCode::UNAUTHORIZED);
}

/// 凍結はトークンも断ち、解除してもそのトークンは戻らない (セッションと同じ)。
#[sqlx::test]
async fn freezing_cuts_off_the_app_token(pool: SqlitePool) {
    insert_user(&pool, "admin", "password").await;
    let app = test_app(pool.clone()).await;
    let token = app_login(&app, "admin", "password").await;

    freeze_user(&pool, "admin").await;
    assert_eq!(app_me_status(&app, &token).await, StatusCode::UNAUTHORIZED);
    sqlx::query!("UPDATE users SET frozen = 0 WHERE username = 'admin'")
        .execute(&pool)
        .await
        .expect("テストデータを更新できるはず");
    assert_eq!(app_me_status(&app, &token).await, StatusCode::UNAUTHORIZED);
}

/// アプリでパスワードを変えると、その端末のトークンは残り、ほかの端末のトークンとセッションは断たれる。
#[sqlx::test]
async fn app_password_change_keeps_only_the_changing_token(pool: SqlitePool) {
    insert_user(&pool, "admin", "password").await;
    let app = test_app(pool).await;
    let changing = app_login(&app, "admin", "password").await;
    let other = app_login(&app, "admin", "password").await;
    let cookie = login_and_get_cookie(&app, "admin", "password").await;

    let response = send(
        &app,
        app_request(
            Method::PUT,
            "/api/v1/account/password",
            Some(&changing),
            Some(json!({"currentPassword": "password", "newPassword": "new-password"})),
        ),
    )
    .await;
    assert_eq!(response.status(), StatusCode::NO_CONTENT);

    assert_eq!(app_me_status(&app, &changing).await, StatusCode::OK);
    assert_eq!(app_me_status(&app, &other).await, StatusCode::UNAUTHORIZED);
    let me = send(&app, get("/api/v1/auth/me", Some(&cookie))).await;
    assert_eq!(me.status(), StatusCode::UNAUTHORIZED);
}

/// ウェブでパスワードを変えると、アプリのトークンも断たれる。
#[sqlx::test]
async fn web_password_change_cuts_off_app_tokens(pool: SqlitePool) {
    insert_user(&pool, "admin", "password").await;
    let app = test_app(pool).await;
    let token = app_login(&app, "admin", "password").await;
    let cookie = login_and_get_cookie(&app, "admin", "password").await;

    let response = send(
        &app,
        change_password_request(Some(&cookie), "password", "new-password"),
    )
    .await;
    assert_eq!(response.status(), StatusCode::NO_CONTENT);
    assert_eq!(app_me_status(&app, &token).await, StatusCode::UNAUTHORIZED);
}

/// トークンがあるときはトークンだけで判定し、セッションの Cookie は見ない。
#[sqlx::test]
async fn invalid_token_is_not_rescued_by_a_session_cookie(pool: SqlitePool) {
    let (app, cookie) = logged_in_app(pool).await;

    let response = send(
        &app,
        request(
            Method::GET,
            "/api/v1/auth/me",
            Some(&cookie),
            &[(header::AUTHORIZATION, "Bearer not-a-token")],
            None,
        ),
    )
    .await;
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

/// アプリの preflight には CORS で答え、ほかの出どころを許可として返さない。
/// (許す出どころが1つに固定なので、tower-http はどの要求にもアプリの出どころを返す。ブラウザは
/// 自分の出どころと一致しなければ弾く)
#[sqlx::test]
async fn cors_allows_only_the_app_origin(pool: SqlitePool) {
    let app = test_app(pool).await;

    for origin in [APP_ORIGIN, "https://evil.example"] {
        let response = send(
            &app,
            request(
                Method::OPTIONS,
                "/api/v1/app/auth/login",
                None,
                &[
                    (header::ORIGIN, origin),
                    (header::HOST, "bp.example"),
                    (header::ACCESS_CONTROL_REQUEST_METHOD, "POST"),
                    (
                        header::ACCESS_CONTROL_REQUEST_HEADERS,
                        "authorization,content-type,x-app-version",
                    ),
                ],
                None,
            ),
        )
        .await;
        let allow_headers = response
            .headers()
            .get(header::ACCESS_CONTROL_ALLOW_HEADERS)
            .map(|value| value.to_str().expect("ascii").to_ascii_lowercase());
        assert!(
            allow_headers
                .as_deref()
                .is_some_and(|value| value.contains("x-app-version")),
            "{origin}: 版のヘッダーを通す"
        );
        let allow_origin = response
            .headers()
            .get(header::ACCESS_CONTROL_ALLOW_ORIGIN)
            .map(|value| value.to_str().expect("ascii").to_string());
        assert_eq!(allow_origin.as_deref(), Some(APP_ORIGIN), "{origin}");
        assert!(
            response
                .headers()
                .get(header::ACCESS_CONTROL_ALLOW_CREDENTIALS)
                .is_none(),
            "Cookie は許さない"
        );
    }
}

/// `[mobile_app] min_version` より古い版・版を送らないアプリの呼び出しは、426 で断る。
/// 断りにも CORS のヘッダーを付ける (付けないと、アプリはエラーの中身を読めない)。
/// ウェブ版の呼び出しと、アプリの画面以外 (`/api` の外) は見ない。
#[sqlx::test]
async fn old_app_versions_are_asked_to_update(pool: SqlitePool) {
    let mut config = test_config();
    config.mobile_app.min_version = Some("1.2".parse().expect("版が不正"));
    let app = test_app_with_config(pool, &config).await;

    for (version, expected) in [
        (None, StatusCode::UPGRADE_REQUIRED),
        (Some("1.1.9"), StatusCode::UPGRADE_REQUIRED),
        (Some("abc"), StatusCode::UPGRADE_REQUIRED),
        (Some("1.2"), StatusCode::OK),
        (Some("1.10"), StatusCode::OK),
    ] {
        let mut headers = vec![(header::ORIGIN, APP_ORIGIN), (header::HOST, "bp.example")];
        let name = header::HeaderName::from_static("x-app-version");
        if let Some(version) = version {
            headers.push((name, version));
        }
        let response = send(
            &app,
            request(Method::GET, "/api/v1/auth/providers", None, &headers, None),
        )
        .await;
        assert_eq!(response.status(), expected, "{version:?}");
        assert_eq!(
            response.headers().get(header::ACCESS_CONTROL_ALLOW_ORIGIN),
            Some(&header::HeaderValue::from_static(APP_ORIGIN)),
            "{version:?}"
        );
        if expected == StatusCode::UPGRADE_REQUIRED {
            let body = json_body(response).await;
            assert_eq!(body["error"]["code"], "app_update_required");
        }
    }

    // ウェブ版は版を送らないが、断らない。
    let response = send(
        &app,
        request(
            Method::GET,
            "/api/v1/auth/providers",
            None,
            &[(header::HOST, "bp.example")],
            None,
        ),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
}

/// 設定が無ければ、版を送らないアプリの呼び出しも断らない。
#[sqlx::test]
async fn app_version_is_not_checked_without_min_version(pool: SqlitePool) {
    let app = test_app(pool).await;
    let response = send(
        &app,
        app_request(Method::GET, "/api/v1/auth/providers", None, None),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
}

/// 同一オリジンチェックは、アプリの出どころだけを通し、ほかの別オリジンは今までどおり弾く。
#[sqlx::test]
async fn csrf_check_lets_only_the_app_origin_through(pool: SqlitePool) {
    insert_user(&pool, "admin", "password").await;
    let app = test_app(pool).await;

    for (origin, expected) in [
        (APP_ORIGIN, StatusCode::OK),
        ("https://evil.example", StatusCode::FORBIDDEN),
    ] {
        let response = send(
            &app,
            login_request_with_headers(&[(header::ORIGIN, origin), (header::HOST, "bp.example")]),
        )
        .await;
        assert_eq!(response.status(), expected, "{origin}");
    }
}

async fn app_google_login_response(app: &Router, code: &str) -> Response<Body> {
    send(
        app,
        app_request(
            Method::POST,
            "/api/v1/app/auth/google",
            None,
            Some(json!({"serverAuthCode": code})),
        ),
    )
    .await
}

/// アプリの Google ログインは、認可コードを Web 用のクライアントで引き換え、PKCE を付けない
/// (docs/mobile-app.md)。作られた利用者のトークンで API を使える。
#[sqlx::test]
async fn app_google_login_exchanges_the_code_and_returns_a_token(pool: SqlitePool) {
    let mock = start_google_mock(google_claims("sub-1", "newuser@example.com", true)).await;
    let app = test_app_with_google_login(pool.clone(), &mock).await;

    let response = app_google_login_response(&app, "server-code").await;
    assert_eq!(response.status(), StatusCode::OK);
    assert!(response.headers().get(header::SET_COOKIE).is_none());
    let body = json_body(response).await;
    assert_eq!(body["user"]["username"], "newuser@example.com");
    let token = body["token"].as_str().expect("token should be a string");
    assert_eq!(app_me_status(&app, token).await, StatusCode::OK);

    let token_request = mock
        .captured_token_request
        .lock()
        .expect("mutex lock should not be poisoned")
        .clone()
        .expect("token endpoint should have been called");
    assert_eq!(token_request["code"], "server-code");
    assert_eq!(token_request["client_id"], "test-client-id");
    assert!(!token_request.contains_key("code_verifier"));
    assert_eq!(
        count_identities(&pool, LoginProvider::Google, Some("sub-1")).await,
        1
    );
}

#[sqlx::test]
async fn app_google_login_rejects_what_the_web_login_rejects(pool: SqlitePool) {
    insert_user(&pool, "frozen@example.com", "password").await;
    freeze_user(&pool, "frozen@example.com").await;
    let mock = start_google_mock_with_sequence(vec![
        google_claims("sub-unverified", "user@example.com", false),
        google_claims("sub-frozen", "frozen@example.com", true),
        google_claims("sub-original", "reassigned@example.com", true),
        google_claims("sub-new", "reassigned@example.com", true),
    ])
    .await;
    let app = test_app_with_google_login(pool.clone(), &mock).await;

    for (name, status, code) in [
        (
            "unverified email",
            StatusCode::UNAUTHORIZED,
            "google_email_untrusted",
        ),
        ("frozen account", StatusCode::FORBIDDEN, "account_frozen"),
    ] {
        let response = app_google_login_response(&app, "code").await;
        assert_error_case(response, status, code, name).await;
    }
    assert_eq!(
        app_google_login_response(&app, "code").await.status(),
        StatusCode::OK
    );
    let response = app_google_login_response(&app, "code").await;
    assert_error(response, StatusCode::CONFLICT, "external_account_conflict").await;
    assert_eq!(
        count_identities(&pool, LoginProvider::Google, Some("sub-frozen")).await,
        0
    );
}

#[sqlx::test]
async fn app_google_login_is_unavailable_when_google_login_is_disabled(pool: SqlitePool) {
    let app = test_app(pool).await;
    let response = app_google_login_response(&app, "code").await;
    assert_error(
        response,
        StatusCode::SERVICE_UNAVAILABLE,
        "login_provider_disabled",
    )
    .await;
}

/// アプリの LINE ログインは、ID トークンを発行した nonce 付きで確かめ、アクセストークンの持ち主を
/// 照らし合わせてから、アクセストークンを暗号化して保存する (docs/mobile-app.md)。
#[sqlx::test]
async fn app_line_login_verifies_the_tokens_and_keeps_the_access_token(pool: SqlitePool) {
    let mock = start_line_mock(line_id_token("U1", Some("newuser@example.com"))).await;
    let app = test_app_with_line_login(pool.clone(), &mock).await;
    let nonce = app_login_nonce(&app).await;

    let response = app_line_login_response(&app, &nonce).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert!(response.headers().get(header::SET_COOKIE).is_none());
    let body = json_body(response).await;
    assert_eq!(body["user"]["username"], "newuser@example.com");
    let token = body["token"].as_str().expect("token should be a string");
    assert_eq!(app_me_status(&app, token).await, StatusCode::OK);

    let verify_request = mock
        .verify_requests
        .lock()
        .expect("mutex lock should not be poisoned")[0]
        .clone();
    assert_eq!(verify_request["id_token"], "sdk-id-token");
    assert_eq!(verify_request["client_id"], "test-channel-id");
    assert_eq!(verify_request["nonce"], nonce);
    assert_eq!(
        *mock
            .profile_requests
            .lock()
            .expect("mutex lock should not be poisoned"),
        [format!("Bearer {LINE_APP_ACCESS_TOKEN}")]
    );
    let sealed = identity_tokens(&pool, LoginProvider::Line, "U1")
        .await
        .access_token
        .expect("access token should be kept");
    assert!(
        !sealed.contains(LINE_APP_ACCESS_TOKEN),
        "暗号化して保存する"
    );
    assert_eq!(
        identity_tokens(&pool, LoginProvider::Line, "U1")
            .await
            .refresh_token,
        None
    );
}

/// 発行していない・使用済みの nonce では、LINE に問い合わせずに断る。
#[sqlx::test]
async fn app_line_login_accepts_each_issued_nonce_only_once(pool: SqlitePool) {
    let mock = start_line_mock(line_id_token("U1", Some("user@example.com"))).await;
    let app = test_app_with_line_login(pool.clone(), &mock).await;

    let response = app_line_login_response(&app, "not-issued").await;
    assert_error(response, StatusCode::UNAUTHORIZED, "external_login_failed").await;
    assert!(
        mock.verify_requests
            .lock()
            .expect("mutex lock should not be poisoned")
            .is_empty()
    );

    let nonce = app_login_nonce(&app).await;
    assert_eq!(
        app_line_login_response(&app, &nonce).await.status(),
        StatusCode::OK
    );
    assert_eq!(
        app_line_login_response(&app, &nonce).await.status(),
        StatusCode::UNAUTHORIZED
    );
}

/// 別の人のアクセストークンは、保存もログインもさせない (退会のときにその人の連携を取り消さないため)。
#[sqlx::test]
async fn app_line_login_rejects_an_access_token_of_someone_else(pool: SqlitePool) {
    let mock = start_line_mock(line_id_token("U1", Some("user@example.com"))).await;
    *mock
        .profile_user_id
        .lock()
        .expect("mutex lock should not be poisoned") = "U2".to_string();
    let app = test_app_with_line_login(pool.clone(), &mock).await;
    let nonce = app_login_nonce(&app).await;

    let response = app_line_login_response(&app, &nonce).await;
    assert_error(response, StatusCode::UNAUTHORIZED, "external_login_failed").await;
    assert_eq!(count_identities(&pool, LoginProvider::Line, None).await, 0);
}

/// メールアドレスの提供を断られたら、ウェブと同じコードで断り、アカウントを作らない。
#[sqlx::test]
async fn app_line_login_requires_the_email_address(pool: SqlitePool) {
    let mock = start_line_mock(line_id_token("U1", None)).await;
    let app = test_app_with_line_login(pool.clone(), &mock).await;
    let nonce = app_login_nonce(&app).await;

    let response = app_line_login_response(&app, &nonce).await;
    assert_error(response, StatusCode::UNAUTHORIZED, "line_email_required").await;
    assert_eq!(count_users(&pool).await, 0);
}

#[sqlx::test]
async fn app_line_login_is_unavailable_when_line_login_is_disabled(pool: SqlitePool) {
    let app = test_app(pool).await;
    let nonce = app_login_nonce(&app).await;
    let response = app_line_login_response(&app, &nonce).await;
    assert_error(
        response,
        StatusCode::SERVICE_UNAVAILABLE,
        "login_provider_disabled",
    )
    .await;
}

/// アプリでだけログインした人は、保存したアクセストークンで LINE との連携を取り消す。
#[sqlx::test]
async fn unlink_revokes_the_line_link_with_the_app_access_token(pool: SqlitePool) {
    insert_user(&pool, "kyoko@example.com", "password").await;
    let mock = start_line_mock(line_id_token("U1", Some("kyoko@example.com"))).await;
    let app = test_app_with_line_login(pool.clone(), &mock).await;
    let nonce = app_login_nonce(&app).await;
    let body = json_body(app_line_login_response(&app, &nonce).await).await;
    let token = body["token"].as_str().expect("token should be a string");

    let response = send(
        &app,
        app_request(
            Method::DELETE,
            "/api/v1/account/identities/line",
            Some(token),
            None,
        ),
    )
    .await;

    assert_eq!(response.status(), StatusCode::NO_CONTENT);
    assert!(
        mock.token_requests
            .lock()
            .expect("mutex lock should not be poisoned")
            .is_empty(),
        "リフレッシュトークンが無いので取り直さない"
    );
    assert_eq!(
        *mock
            .deauthorize_requests
            .lock()
            .expect("mutex lock should not be poisoned"),
        [(
            "Bearer channel-access-token".to_string(),
            json!({ "userAccessToken": LINE_APP_ACCESS_TOKEN })
        )]
    );
}
