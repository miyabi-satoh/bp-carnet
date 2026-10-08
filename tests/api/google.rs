//! Google のログイン (docs/authentication.md)。

use super::*;

#[sqlx::test]
async fn auth_providers_reports_google_disabled_by_default(pool: SqlitePool) {
    let app = test_app(pool).await;
    assert_eq!(auth_providers(app).await["googleEnabled"], false);
}

#[sqlx::test]
async fn auth_providers_reports_google_enabled_when_configured(pool: SqlitePool) {
    let mock = start_google_mock(google_claims("sub-1", "user@example.com", true)).await;
    let app = test_app_with_google_login(pool, &mock).await;
    assert_eq!(auth_providers(app).await["googleEnabled"], true);
}

#[sqlx::test]
async fn google_login_redirects_to_login_with_error_when_disabled(pool: SqlitePool) {
    let app = test_app(pool).await;
    let response = send(&app, get("/api/v1/auth/google/login", None)).await;
    assert_eq!(response.status(), StatusCode::SEE_OTHER);
    assert_eq!(
        location_header(&response),
        "/login?oauthError=google_disabled"
    );
}

#[sqlx::test]
async fn google_login_redirects_to_google_and_sets_state_cookie(pool: SqlitePool) {
    let mock = start_google_mock(google_claims("sub-1", "user@example.com", true)).await;
    let app = test_app_with_google_login(pool, &mock).await;

    let response = send(&app, get("/api/v1/auth/google/login", None)).await;
    assert_eq!(response.status(), StatusCode::SEE_OTHER);

    let location = location_header(&response);
    assert!(location.starts_with("https://accounts.google.com/o/oauth2/v2/auth?"));
    assert_eq!(query_param(&location, "client_id"), "test-client-id");
    assert_eq!(
        query_param(&location, "redirect_uri"),
        format!(
            "http://localhost:5173{}",
            bp_carnet::google_login::CALLBACK_PATH
        )
    );
    assert_eq!(query_param(&location, "response_type"), "code");
    assert_eq!(query_param(&location, "scope"), "openid email profile");
    assert_eq!(query_param(&location, "code_challenge_method"), "S256");
    assert!(!query_param(&location, "code_challenge").is_empty());
    assert!(!query_param(&location, "state").is_empty());

    let cookies = set_cookie_values(&response);
    let state_cookie =
        find_set_cookie(&cookies, "oauth_state").expect("oauth_state cookie should be set");
    assert!(state_cookie.contains("HttpOnly"));
    assert!(state_cookie.contains("SameSite=Lax"));
    assert!(state_cookie.contains("Path=/api/v1/auth/google"));
}

/// `google_login` が発行した `code_challenge` と、コールバック経由でモックの `/token` に
/// 実際に送られた `code_verifier` が PKCE (S256) の関係を満たしていることを確認する。
#[sqlx::test]
async fn google_login_pkce_challenge_matches_verifier_sent_at_callback(pool: SqlitePool) {
    let mock = start_google_mock(google_claims("sub-1", "user@example.com", true)).await;
    let app = test_app_with_google_login(pool, &mock).await;

    let login_response = send(&app, get("/api/v1/auth/google/login", None)).await;
    let location = location_header(&login_response);
    let code_challenge = query_param(&location, "code_challenge");
    let state = query_param(&location, "state");
    let state_cookie = cookie_pair(&login_response, "oauth_state");

    let callback_response = send(
        &app,
        get(
            &format!("/api/v1/auth/google/callback?code=test-code&state={state}"),
            Some(&state_cookie),
        ),
    )
    .await;
    assert_eq!(callback_response.status(), StatusCode::SEE_OTHER);
    assert_eq!(location_header(&callback_response), "/");

    let captured = mock
        .captured_token_request
        .lock()
        .expect("mutex lock should not be poisoned")
        .clone()
        .expect("token endpoint should have been called");
    let code_verifier = captured
        .get("code_verifier")
        .expect("token request should include code_verifier");
    assert_eq!(
        bp_carnet::external_login::code_challenge_s256(code_verifier),
        code_challenge
    );
}

#[sqlx::test]
async fn google_callback_rejects_state_mismatch(pool: SqlitePool) {
    let mock = start_google_mock(google_claims("sub-1", "user@example.com", true)).await;
    let app = test_app_with_google_login(pool, &mock).await;

    let login_response = send(&app, get("/api/v1/auth/google/login", None)).await;
    let state_cookie = cookie_pair(&login_response, "oauth_state");

    let response = send(
        &app,
        get(
            "/api/v1/auth/google/callback?code=test-code&state=wrong-state",
            Some(&state_cookie),
        ),
    )
    .await;
    assert_eq!(response.status(), StatusCode::SEE_OTHER);
    assert_eq!(
        location_header(&response),
        "/login?oauthError=google_failed"
    );
    let cookies = set_cookie_values(&response);
    let cleared = find_set_cookie(&cookies, "oauth_state")
        .expect("oauth_state cookie should still be present (cleared)");
    assert!(cleared.contains("Max-Age=0"));
}

#[sqlx::test]
async fn google_callback_without_state_cookie_is_rejected(pool: SqlitePool) {
    let mock = start_google_mock(google_claims("sub-1", "user@example.com", true)).await;
    let app = test_app_with_google_login(pool, &mock).await;

    let response = send(
        &app,
        get(
            "/api/v1/auth/google/callback?code=test-code&state=some-state",
            None,
        ),
    )
    .await;
    assert_eq!(response.status(), StatusCode::SEE_OTHER);
    assert_eq!(
        location_header(&response),
        "/login?oauthError=google_failed"
    );
}

#[sqlx::test]
async fn google_callback_with_provider_error_is_rejected(pool: SqlitePool) {
    let mock = start_google_mock(google_claims("sub-1", "user@example.com", true)).await;
    let app = test_app_with_google_login(pool, &mock).await;

    let response = send(
        &app,
        get("/api/v1/auth/google/callback?error=access_denied", None),
    )
    .await;
    assert_eq!(response.status(), StatusCode::SEE_OTHER);
    assert_eq!(
        location_header(&response),
        "/login?oauthError=google_failed"
    );
}

/// Google が持ち主を保証しないメールは、既存のアカウントにも結ばず、新しいアカウントも作らない
/// (docs/authentication.md)。どのメールを保証とみなすかは `GoogleIdToken::email_is_trusted` の単体テスト。
#[sqlx::test]
async fn google_callback_rejects_emails_google_does_not_vouch_for(pool: SqlitePool) {
    insert_user(&pool, "victim@example.com", "password").await;
    // Gmail でも Google Workspace でもないメールで作った Google アカウント。
    let mock = start_google_mock(
        json!({ "sub": "sub-2", "email": "victim@example.com", "email_verified": true }),
    )
    .await;
    let app = test_app_with_google_login(pool.clone(), &mock).await;

    let response = run_login_flow(&app, LoginProvider::Google).await;
    assert_eq!(response.status(), StatusCode::SEE_OTHER);
    assert_eq!(
        location_header(&response),
        "/login?oauthError=google_email_untrusted"
    );
    assert_eq!(
        count_identities(&pool, LoginProvider::Google, None).await,
        0
    );
    let users: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM users")
        .fetch_one(&pool)
        .await
        .expect("count users");
    assert_eq!(users, 1);
}

#[sqlx::test]
async fn google_callback_creates_new_user_and_session_on_first_login(pool: SqlitePool) {
    let mut claims = google_claims("sub-1", "newuser@example.com", true);
    claims["name"] = json!("Google の名前");
    claims["picture"] = json!("https://lh3.googleusercontent.com/a/abc");
    let mock = start_google_mock(claims).await;
    let app = test_app_with_google_login(pool.clone(), &mock).await;

    let response = run_login_flow(&app, LoginProvider::Google).await;
    assert_eq!(response.status(), StatusCode::SEE_OTHER);
    assert_eq!(location_header(&response), "/");

    let session_cookie_value = session_cookie(&response);

    let user = sqlx::query!(
        r#"SELECT id AS "id!: i64", username FROM users WHERE username = ?"#,
        "newuser@example.com"
    )
    .fetch_one(&pool)
    .await
    .expect("user should have been created");

    let identity = sqlx::query!(
        r#"SELECT user_id AS "user_id!: i64" FROM oauth_identities WHERE provider = 'google' AND subject = ?"#,
        "sub-1"
    )
    .fetch_one(&pool)
    .await
    .expect("oauth identity should have been created");
    assert_eq!(identity.user_id, user.id);

    // セッションが確立され、/auth/me がそのユーザーを返すことを確認する。
    let me_response = send(&app, get("/api/v1/auth/me", Some(&session_cookie_value))).await;
    assert_eq!(me_response.status(), StatusCode::OK);
    let me = json_body(me_response).await;
    assert_eq!(me["username"], "newuser@example.com");
    assert_eq!(me["displayName"], "Google の名前");
    assert_eq!(me["avatarUrl"], "https://lh3.googleusercontent.com/a/abc");
}

/// Google ログインでもセッションに現在の世代を持たせる (docs/authentication.md)。持たせないと、
/// 一度パスワードを変えたユーザーが Google で入り直しても、次のアクセスで断たれてしまう。
#[sqlx::test]
async fn google_callback_stores_the_current_session_generation(pool: SqlitePool) {
    insert_user(&pool, "shared@example.com", "password").await;
    // パスワードを変更したことがあるユーザーを模して、世代を進めておく。
    sqlx::query!("UPDATE users SET session_generation = 3 WHERE username = 'shared@example.com'")
        .execute(&pool)
        .await
        .expect("failed to bump the session generation");

    let mock = start_google_mock(google_claims("sub-1", "shared@example.com", true)).await;
    let app = test_app_with_google_login(pool, &mock).await;

    let response = run_login_flow(&app, LoginProvider::Google).await;
    assert_eq!(location_header(&response), "/");
    let session_cookie_value = session_cookie(&response);

    // 世代を持たないセッションは 0 として扱われて 401 になる。通ることが、3 を持たせた証拠。
    let me = send(&app, get("/api/v1/auth/me", Some(&session_cookie_value))).await;
    assert_eq!(me.status(), StatusCode::OK);
}

/// 同じメールアドレスが別の Google アカウント (別の `sub`) に割り当て直された場合、
/// 既に連携済みのユーザーには統合しない。
#[sqlx::test]
async fn google_callback_does_not_link_account_already_linked_to_another_google_account(
    pool: SqlitePool,
) {
    let mock = start_google_mock_with_sequence(vec![
        google_claims("sub-original", "reassigned@example.com", true),
        google_claims("sub-new", "reassigned@example.com", true),
    ])
    .await;
    let app = test_app_with_google_login(pool.clone(), &mock).await;

    let first = run_login_flow(&app, LoginProvider::Google).await;
    assert_eq!(location_header(&first), "/");

    let second = run_login_flow(&app, LoginProvider::Google).await;
    assert_eq!(
        location_header(&second),
        "/login?oauthError=google_account_conflict"
    );

    let identity_count = count_identities(&pool, LoginProvider::Google, Some("sub-new")).await;
    assert_eq!(
        identity_count, 0,
        "連携済みのユーザーに別の sub を紐付けない"
    );
}
