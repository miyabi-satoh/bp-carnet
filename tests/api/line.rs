//! LINE のログイン (docs/authentication.md)。

use super::*;

#[sqlx::test]
async fn auth_providers_reports_whether_line_is_enabled(pool: SqlitePool) {
    let app = test_app(pool.clone()).await;
    assert_eq!(auth_providers(app).await["lineEnabled"], false);

    let mock = start_line_mock(line_id_token("U1", Some("user@example.com"))).await;
    let app = test_app_with_line_login(pool, &mock).await;
    assert_eq!(auth_providers(app).await["lineEnabled"], true);
}

#[sqlx::test]
async fn line_login_redirects_to_login_with_error_when_disabled(pool: SqlitePool) {
    let app = test_app(pool).await;
    let response = send(&app, get("/api/v1/auth/line/login", None)).await;
    assert_eq!(response.status(), StatusCode::SEE_OTHER);
    assert_eq!(
        location_header(&response),
        "/login?oauthError=line_disabled"
    );
}

#[sqlx::test]
async fn line_callback_redirects_to_login_with_error_when_disabled(pool: SqlitePool) {
    let app = test_app(pool).await;
    let response = send(&app, get("/api/v1/auth/line/callback?code=c&state=s", None)).await;
    assert_eq!(
        location_header(&response),
        "/login?oauthError=line_disabled"
    );
}

#[sqlx::test]
async fn line_login_redirects_to_line_and_sets_its_own_state_cookie(pool: SqlitePool) {
    let mock = start_line_mock(line_id_token("U1", Some("user@example.com"))).await;
    let app = test_app_with_line_login(pool, &mock).await;

    let response = send(&app, get("/api/v1/auth/line/login", None)).await;
    assert_eq!(response.status(), StatusCode::SEE_OTHER);

    let location = location_header(&response);
    assert!(location.starts_with("https://access.line.me/oauth2/v2.1/authorize?"));
    assert_eq!(query_param(&location, "client_id"), "test-channel-id");
    assert_eq!(
        query_param(&location, "redirect_uri"),
        format!("http://localhost:5173{}", line_login::CALLBACK_PATH)
    );
    assert_eq!(query_param(&location, "response_type"), "code");
    assert_eq!(query_param(&location, "scope"), "openid profile email");
    assert_eq!(query_param(&location, "code_challenge_method"), "S256");
    assert!(!query_param(&location, "code_challenge").is_empty());
    assert!(!query_param(&location, "nonce").is_empty());
    // LINE は state に英数字だけを求める。
    let state = query_param(&location, "state");
    assert!(!state.is_empty() && state.chars().all(|c| c.is_ascii_alphanumeric()));

    let cookies = set_cookie_values(&response);
    let state_cookie = find_set_cookie(&cookies, "line_oauth_state")
        .expect("line_oauth_state cookie should be set");
    assert!(state_cookie.contains("HttpOnly"));
    assert!(state_cookie.contains("SameSite=Lax"));
    assert!(state_cookie.contains("Path=/api/v1/auth/line"));
    assert!(find_set_cookie(&cookies, "oauth_state").is_none());
}

/// コールバックで、ログインの開始時の `code_verifier` をトークンの交換に、`nonce` を ID トークンの
/// 検証に送る。チャネルシークレットもトークンの交換に要る。
#[sqlx::test]
async fn line_callback_sends_the_pkce_verifier_and_the_nonce(pool: SqlitePool) {
    let mock = start_line_mock(line_id_token("U1", Some("user@example.com"))).await;
    let app = test_app_with_line_login(pool, &mock).await;

    let login_response = send(&app, get("/api/v1/auth/line/login", None)).await;
    let location = location_header(&login_response);
    let state = query_param(&location, "state");
    let state_cookie = cookie_pair(&login_response, "line_oauth_state");

    let response = send(
        &app,
        get(
            &format!("/api/v1/auth/line/callback?code=test-code&state={state}"),
            Some(&state_cookie),
        ),
    )
    .await;
    assert_eq!(location_header(&response), "/");
    let cookies = set_cookie_values(&response);
    let cleared = find_set_cookie(&cookies, "line_oauth_state")
        .expect("line_oauth_state cookie should be cleared");
    assert!(cleared.contains("Max-Age=0"));

    let token_request = mock
        .token_requests
        .lock()
        .expect("mutex lock should not be poisoned")[0]
        .clone();
    assert_eq!(token_request["grant_type"], "authorization_code");
    assert_eq!(token_request["code"], "test-code");
    assert_eq!(token_request["client_secret"], "test-channel-secret");
    assert_eq!(
        bp_carnet::external_login::code_challenge_s256(&token_request["code_verifier"]),
        query_param(&location, "code_challenge")
    );

    let verify_request = mock
        .verify_requests
        .lock()
        .expect("mutex lock should not be poisoned")[0]
        .clone();
    assert_eq!(verify_request["id_token"], "line-id-token");
    assert_eq!(verify_request["client_id"], "test-channel-id");
    assert_eq!(verify_request["nonce"], query_param(&location, "nonce"));
}

#[sqlx::test]
async fn line_callback_rejects_a_state_mismatch(pool: SqlitePool) {
    let mock = start_line_mock(line_id_token("U1", Some("user@example.com"))).await;
    let app = test_app_with_line_login(pool.clone(), &mock).await;

    let login_response = send(&app, get("/api/v1/auth/line/login", None)).await;
    let state_cookie = cookie_pair(&login_response, "line_oauth_state");

    let response = send(
        &app,
        get(
            "/api/v1/auth/line/callback?code=test-code&state=wrongstate",
            Some(&state_cookie),
        ),
    )
    .await;
    assert_eq!(location_header(&response), "/login?oauthError=line_failed");
    assert!(
        mock.token_requests
            .lock()
            .expect("mutex lock should not be poisoned")
            .is_empty()
    );
    assert_eq!(count_users(&pool).await, 0);
}

/// Google の state の Cookie では LINE のコールバックを通さない。
#[sqlx::test]
async fn line_callback_does_not_accept_the_google_state_cookie(pool: SqlitePool) {
    let mock = start_line_mock(line_id_token("U1", Some("user@example.com"))).await;
    let app = test_app_with_line_login(pool, &mock).await;

    let response = send(
        &app,
        get(
            "/api/v1/auth/line/callback?code=test-code&state=abc",
            Some("oauth_state=abc.verifier"),
        ),
    )
    .await;
    assert_eq!(location_header(&response), "/login?oauthError=line_failed");
}

/// 同意画面でキャンセルされると LINE は `error` を付けて戻す。
#[sqlx::test]
async fn line_callback_with_provider_error_is_rejected(pool: SqlitePool) {
    let mock = start_line_mock(line_id_token("U1", Some("user@example.com"))).await;
    let app = test_app_with_line_login(pool, &mock).await;

    let response = send(
        &app,
        get(
            "/api/v1/auth/line/callback?error=ACCESS_DENIED&state=abc",
            None,
        ),
    )
    .await;
    assert_eq!(location_header(&response), "/login?oauthError=line_failed");
}

/// メールアドレスの提供を断られたら、アカウントを作らずに案内する (docs/authentication.md)。
#[sqlx::test]
async fn line_callback_requires_the_email_address(pool: SqlitePool) {
    let mock = start_line_mock(line_id_token("U1", None)).await;
    let app = test_app_with_line_login(pool.clone(), &mock).await;

    let response = run_login_flow(&app, LoginProvider::Line).await;

    assert_eq!(
        location_header(&response),
        "/login?oauthError=line_email_required"
    );
    assert_eq!(count_users(&pool).await, 0);
    assert_eq!(count_identities(&pool, LoginProvider::Line, None).await, 0);
}

/// 初めての LINE ログインでアカウントを作り、ログインさせる。ユーザーIDはメールアドレス (小文字)。
/// 連携の取り消しに使うリフレッシュトークンは、暗号化して保存する。紐付けの決まりの場合ごとの確かめは
/// `oauth_identity` の単体テスト。
#[sqlx::test]
async fn line_callback_creates_a_user_and_keeps_the_sealed_refresh_token(pool: SqlitePool) {
    let mock = start_line_mock(json!({
        "sub": "U1",
        "email": "NewUser@Example.com",
        "name": "LINE の名前",
        "picture": "https://profile.line-scdn.net/abc"
    }))
    .await;
    let app = test_app_with_line_login(pool.clone(), &mock).await;

    let response = run_login_flow(&app, LoginProvider::Line).await;
    assert_eq!(location_header(&response), "/");

    let me = send(
        &app,
        get("/api/v1/auth/me", Some(&session_cookie(&response))),
    )
    .await;
    assert_eq!(me.status(), StatusCode::OK);
    let me = json_body(me).await;
    assert_eq!(me["username"], "newuser@example.com");
    assert_eq!(me["displayName"], "LINE の名前");
    assert_eq!(me["avatarUrl"], "https://profile.line-scdn.net/abc");
    assert_eq!(me["passwordUsable"], false);
    let (terms_version, agreed_at) = terms_agreement(&pool, "newuser@example.com").await;
    assert_eq!(terms_version.as_deref(), Some(bp_carnet::terms::VERSION));
    assert!(agreed_at.is_some());

    let sealed = identity_tokens(&pool, LoginProvider::Line, "U1")
        .await
        .refresh_token
        .expect("refresh token should be stored");
    assert!(!sealed.contains(LINE_REFRESH_TOKEN));
}

#[sqlx::test]
async fn line_callback_reuses_the_identity_on_repeat_login(pool: SqlitePool) {
    let mock = start_line_mock(line_id_token("U1", Some("repeat@example.com"))).await;
    let app = test_app_with_line_login(pool.clone(), &mock).await;

    run_login_flow(&app, LoginProvider::Line).await;
    let first = identity_tokens(&pool, LoginProvider::Line, "U1")
        .await
        .refresh_token;
    let second = run_login_flow(&app, LoginProvider::Line).await;

    assert_eq!(location_header(&second), "/");
    assert_eq!(count_users(&pool).await, 1);
    assert_eq!(count_identities(&pool, LoginProvider::Line, None).await, 1);
    // ログインのたびに受け取った値で保存し直す (暗号化の nonce が変わるので値も変わる)。
    assert_ne!(
        identity_tokens(&pool, LoginProvider::Line, "U1")
            .await
            .refresh_token,
        first
    );
}

/// 別の LINE アカウントと連携済みのアカウントには紐付けない。
#[sqlx::test]
async fn line_callback_does_not_link_an_account_linked_to_another_line_account(pool: SqlitePool) {
    let mock = start_line_mock(line_id_token("U1", Some("shared@example.com"))).await;
    let app = test_app_with_line_login(pool.clone(), &mock).await;
    run_login_flow(&app, LoginProvider::Line).await;

    *mock
        .id_token
        .lock()
        .expect("mutex lock should not be poisoned") =
        line_id_token("U2", Some("shared@example.com"));
    let response = run_login_flow(&app, LoginProvider::Line).await;

    assert_eq!(
        location_header(&response),
        "/login?oauthError=line_account_conflict"
    );
    assert_eq!(count_identities(&pool, LoginProvider::Line, None).await, 1);
}

#[sqlx::test]
async fn line_callback_does_not_link_a_frozen_account(pool: SqlitePool) {
    insert_user(&pool, "frozen@example.com", "password").await;
    freeze_user(&pool, "frozen@example.com").await;
    let mock = start_line_mock(line_id_token("U1", Some("frozen@example.com"))).await;
    let app = test_app_with_line_login(pool.clone(), &mock).await;

    let response = run_login_flow(&app, LoginProvider::Line).await;

    assert_eq!(
        location_header(&response),
        "/login?oauthError=account_frozen"
    );
    assert_eq!(count_identities(&pool, LoginProvider::Line, None).await, 0);
}

#[sqlx::test]
async fn line_callback_cancels_a_scheduled_deletion(pool: SqlitePool) {
    let mock = start_line_mock(line_id_token("U1", Some("scheduled@example.com"))).await;
    let app = test_app_with_line_login(pool.clone(), &mock).await;
    run_login_flow(&app, LoginProvider::Line).await;
    set_deletion_scheduled(
        &pool,
        "scheduled@example.com",
        "2100-01-01T00:00:00.000Z",
        "user",
    )
    .await;

    let response = run_login_flow(&app, LoginProvider::Line).await;

    assert_eq!(location_header(&response), "/?deletionCancelled=1");
    assert_eq!(
        deletion_scheduled_at_of(&pool, "scheduled@example.com").await,
        None
    );
}

/// 猶予期間を過ぎたアカウントを消すとき、LINE との連携を取り消す。保存したリフレッシュトークンで
/// アクセストークンを取り直し、ステートレスチャネルアクセストークンを添えて送る。
#[sqlx::test]
async fn purge_revokes_the_line_link_before_deleting(pool: SqlitePool) {
    let mock = start_line_mock(line_id_token("U1", Some("leaving@example.com"))).await;
    let app = test_app_with_line_login(pool.clone(), &mock).await;
    run_login_flow(&app, LoginProvider::Line).await;
    set_deletion_scheduled(
        &pool,
        "leaving@example.com",
        "2000-01-01T00:00:00.000Z",
        "user",
    )
    .await;

    let deleted = bp_carnet::account::purge_expired(
        &pool,
        &line_client(&mock),
        &AppleLoginClient::disabled(),
    )
    .await
    .expect("物理削除できるはず");

    assert_eq!(deleted, 1);
    assert_eq!(count_users(&pool).await, 0);
    let token_requests = mock
        .token_requests
        .lock()
        .expect("mutex lock should not be poisoned")
        .clone();
    let refresh = token_requests.last().expect("refresh should be requested");
    assert_eq!(refresh["grant_type"], "refresh_token");
    assert_eq!(refresh["refresh_token"], LINE_REFRESH_TOKEN);
    assert_eq!(refresh["client_secret"], "test-channel-secret");
    let channel_token = mock
        .channel_token_requests
        .lock()
        .expect("mutex lock should not be poisoned")[0]
        .clone();
    assert_eq!(channel_token["grant_type"], "client_credentials");
    assert_eq!(channel_token["client_id"], "test-channel-id");
    assert_eq!(
        *mock
            .deauthorize_requests
            .lock()
            .expect("mutex lock should not be poisoned"),
        [(
            "Bearer channel-access-token".to_string(),
            json!({ "userAccessToken": "refreshed-access-token" })
        )]
    );
}

/// 取り消せなくても (トークンの期限切れ等)、アカウントの削除は止めない。
#[sqlx::test]
async fn purge_deletes_even_when_the_line_link_cannot_be_revoked(pool: SqlitePool) {
    let mock = start_line_mock(line_id_token("U1", Some("leaving@example.com"))).await;
    let app = test_app_with_line_login(pool.clone(), &mock).await;
    run_login_flow(&app, LoginProvider::Line).await;
    set_deletion_scheduled(
        &pool,
        "leaving@example.com",
        "2000-01-01T00:00:00.000Z",
        "user",
    )
    .await;
    *mock
        .deauthorize_status
        .lock()
        .expect("mutex lock should not be poisoned") = StatusCode::BAD_REQUEST;

    let deleted = bp_carnet::account::purge_expired(
        &pool,
        &line_client(&mock),
        &AppleLoginClient::disabled(),
    )
    .await
    .expect("物理削除できるはず");

    assert_eq!(deleted, 1);
    assert_eq!(count_users(&pool).await, 0);
}

/// 猶予期間中のアカウントは、連携も取り消さない。
#[sqlx::test]
async fn purge_leaves_the_line_link_of_an_account_in_the_grace_period(pool: SqlitePool) {
    let mock = start_line_mock(line_id_token("U1", Some("staying@example.com"))).await;
    let app = test_app_with_line_login(pool.clone(), &mock).await;
    run_login_flow(&app, LoginProvider::Line).await;
    set_deletion_scheduled(
        &pool,
        "staying@example.com",
        "2100-01-01T00:00:00.000Z",
        "user",
    )
    .await;

    let deleted = bp_carnet::account::purge_expired(
        &pool,
        &line_client(&mock),
        &AppleLoginClient::disabled(),
    )
    .await
    .expect("物理削除できるはず");

    assert_eq!(deleted, 0);
    assert!(
        mock.deauthorize_requests
            .lock()
            .expect("mutex lock should not be poisoned")
            .is_empty()
    );
}
