//! Sign in with Apple (docs/authentication.md)。

use super::*;

const APPLE_SERVICE_ID: &str = "com.example.bp.web";

/// Apple の `/token`・`/revoke` を模すローカルサーバー。`/token` は、`claims` に引き換えの
/// `client_id` (`aud`) と今の時刻 (`iat`・`exp`) を足した ID トークンを返す。
struct AppleMock {
    base_url: String,
    /// 返す ID トークンのクレーム (`sub`・`email`・`email_verified`・`nonce`)。
    claims: Arc<Mutex<Value>>,
    token_requests: Arc<Mutex<Vec<HashMap<String, String>>>>,
    revoke_requests: Arc<Mutex<Vec<HashMap<String, String>>>>,
}

/// ウェブとアプリの両方のログインを受け付ける設定。
fn apple_settings() -> apple_login::Settings {
    apple_login::Settings {
        team_id: "TEAM123456".to_string(),
        key_id: "KEY1234567".to_string(),
        private_key_pem: APPLE_TEST_PRIVATE_KEY.to_string(),
        service_id: Some(APPLE_SERVICE_ID.to_string()),
        app_bundle_id: Some(APPLE_BUNDLE_ID.to_string()),
    }
}

fn apple_client(mock: &AppleMock) -> AppleLoginClient {
    apple_client_with(mock, apple_settings())
}

fn apple_client_with(mock: &AppleMock, settings: apple_login::Settings) -> AppleLoginClient {
    AppleLoginClient::for_test(
        settings,
        format!("http://localhost:5173{}", apple_login::CALLBACK_PATH),
        apple_login::Endpoints {
            authorize: "https://appleid.apple.com/auth/authorize".to_string(),
            token: format!("{}/token", mock.base_url),
            revoke: format!("{}/revoke", mock.base_url),
        },
    )
}

async fn test_app_with_apple_login(pool: SqlitePool, mock: &AppleMock) -> Router {
    let services = Services {
        apple_login: Arc::new(apple_client(mock)),
        ..disabled_services()
    };
    build_test_app(pool, &test_config(), services).await
}

const APPLE_REFRESH_TOKEN: &str = "apple-refresh-token";

fn apple_claims(sub: &str, email: &str) -> Value {
    json!({ "iss": "https://appleid.apple.com", "sub": sub, "email": email, "email_verified": "true" })
}

async fn start_apple_mock(claims: Value) -> AppleMock {
    let mock = AppleMock {
        base_url: String::new(),
        claims: Arc::new(Mutex::new(claims)),
        token_requests: Arc::default(),
        revoke_requests: Arc::default(),
    };
    let claims = mock.claims.clone();
    let token_requests = mock.token_requests.clone();
    let revoke_requests = mock.revoke_requests.clone();
    let app = Router::new()
        .route(
            "/token",
            post_route(move |Form(body): Form<HashMap<String, String>>| {
                let claims = claims.clone();
                let token_requests = token_requests.clone();
                async move {
                    let now = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .expect("clock")
                        .as_secs();
                    let mut claims = claims
                        .lock()
                        .expect("mutex lock should not be poisoned")
                        .clone();
                    claims["aud"] = json!(body["client_id"]);
                    claims["iat"] = json!(now);
                    claims["exp"] = json!(now + 600);
                    let id_token = unsigned_jwt(&claims);
                    token_requests
                        .lock()
                        .expect("mutex lock should not be poisoned")
                        .push(body);
                    axum::Json(json!({
                        "access_token": "apple-access-token",
                        "token_type": "bearer",
                        "expires_in": 3600,
                        "refresh_token": APPLE_REFRESH_TOKEN,
                        "id_token": id_token,
                    }))
                }
            }),
        )
        .route(
            "/revoke",
            post_route(move |Form(body): Form<HashMap<String, String>>| {
                let revoke_requests = revoke_requests.clone();
                async move {
                    revoke_requests
                        .lock()
                        .expect("mutex lock should not be poisoned")
                        .push(body);
                    StatusCode::OK
                }
            }),
        );
    AppleMock {
        base_url: serve_mock(app).await,
        ..mock
    }
}

#[sqlx::test]
async fn auth_providers_reports_apple_for_the_web_and_the_app_separately(pool: SqlitePool) {
    let disabled = auth_providers(test_app(pool.clone()).await).await;
    assert_eq!(disabled["appleEnabled"], false);
    assert_eq!(disabled["appleAppEnabled"], false);

    let mock = start_apple_mock(apple_claims("sub1", "a@example.com")).await;
    let enabled = auth_providers(test_app_with_apple_login(pool.clone(), &mock).await).await;
    assert_eq!(enabled["appleEnabled"], true);
    assert_eq!(enabled["appleAppEnabled"], true);

    // Bundle ID が無ければ、ウェブで使えてもアプリでは使えない。
    let web_only = apple_client_with(
        &mock,
        apple_login::Settings {
            app_bundle_id: None,
            ..apple_settings()
        },
    );
    let services = Services {
        apple_login: Arc::new(web_only),
        ..disabled_services()
    };
    let web_only = auth_providers(build_test_app(pool, &test_config(), services).await).await;
    assert_eq!(web_only["appleEnabled"], true);
    assert_eq!(web_only["appleAppEnabled"], false);
}

/// `client_secret` の JWT のクレーム (署名は `apple_login` の単体テストで確かめる)。
fn apple_client_secret_claims(request: &HashMap<String, String>) -> Value {
    let payload = request["client_secret"]
        .split('.')
        .nth(1)
        .expect("client_secret should be a JWT");
    serde_json::from_slice(&URL_SAFE_NO_PAD.decode(payload).expect("base64url")).expect("JSON")
}

/// ウェブのログインを始め、`state` と `nonce` と state Cookie を返す。モックの ID トークンの
/// `nonce` をこのログインのものにする。
async fn start_apple_web_login(app: &Router, mock: &AppleMock) -> (String, String, String) {
    let response = send(app, get("/api/v1/auth/apple/login", None)).await;
    assert_eq!(response.status(), StatusCode::SEE_OTHER);
    let location = location_header(&response);
    let state = query_param(&location, "state");
    let nonce = query_param(&location, "nonce");
    mock.claims
        .lock()
        .expect("mutex lock should not be poisoned")["nonce"] = json!(nonce);
    (state, nonce, cookie_pair(&response, "apple_oauth_state"))
}

/// アプリのログインの `nonce` を受け取り、モックの ID トークンの `nonce` をそれにする。
async fn start_apple_app_login(app: &Router, mock: &AppleMock) -> String {
    let nonce = app_login_nonce(app).await;
    mock.claims
        .lock()
        .expect("mutex lock should not be poisoned")["nonce"] = json!(nonce);
    nonce
}

/// Apple のサイトからのコールバック (`form_post`) を模す。
async fn apple_callback(app: &Router, cookie: &str, form: &str) -> Response {
    send(
        app,
        Request::builder()
            .method(Method::POST)
            .uri("/api/v1/auth/apple/callback")
            .header(header::COOKIE, cookie)
            .header(header::ORIGIN, "https://appleid.apple.com")
            .header(header::HOST, "localhost:5173")
            .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
            .body(Body::from(form.to_string()))
            .expect("failed to build request"),
    )
    .await
}

fn apple_user_form_value(first_name: &str, last_name: &str) -> String {
    let user = json!({ "name": { "firstName": first_name, "lastName": last_name }, "email": "x@example.com" });
    let mut url = reqwest::Url::parse("http://localhost/").expect("URL");
    url.query_pairs_mut().append_pair("user", &user.to_string());
    url.query()
        .expect("query")
        .trim_start_matches("user=")
        .to_string()
}

async fn display_name_of(pool: &SqlitePool, username: &str) -> Option<String> {
    sqlx::query_scalar!(
        r#"SELECT display_name AS "display_name: String" FROM users WHERE username = ?"#,
        username
    )
    .fetch_one(pool)
    .await
    .expect("failed to read display_name")
}

#[sqlx::test]
async fn apple_login_redirects_to_apple_with_form_post_and_its_own_state_cookie(pool: SqlitePool) {
    let mock = start_apple_mock(apple_claims("A1", "user@example.com")).await;
    let app = test_app_with_apple_login(pool, &mock).await;

    let response = send(&app, get("/api/v1/auth/apple/login", None)).await;
    let location = location_header(&response);
    assert!(location.starts_with("https://appleid.apple.com/auth/authorize?"));
    assert_eq!(query_param(&location, "client_id"), APPLE_SERVICE_ID);
    assert_eq!(query_param(&location, "response_mode"), "form_post");
    assert_eq!(query_param(&location, "scope"), "name email");
    assert!(!query_param(&location, "nonce").is_empty());
    assert!(find_set_cookie(&set_cookie_values(&response), "apple_oauth_state").is_some());
}

#[sqlx::test]
async fn apple_login_redirects_to_login_with_error_when_disabled(pool: SqlitePool) {
    let app = test_app(pool).await;
    let response = send(&app, get("/api/v1/auth/apple/login", None)).await;
    assert_eq!(
        location_header(&response),
        "/login?oauthError=apple_disabled"
    );
}

/// Apple のサイトからの POST でも同一オリジンの確認に止められず、引き換えた ID トークンで
/// 利用者を作ってログインさせる。名前は最初の承認のときの `user` から入れ、リフレッシュトークンは
/// 暗号化して保存する。
#[sqlx::test]
async fn apple_callback_creates_a_user_with_the_name_from_the_first_authorization(
    pool: SqlitePool,
) {
    let mock = start_apple_mock(apple_claims("A1", "newuser@privaterelay.appleid.com")).await;
    let app = test_app_with_apple_login(pool.clone(), &mock).await;
    let (state, _, cookie) = start_apple_web_login(&app, &mock).await;

    let response = apple_callback(
        &app,
        &cookie,
        &format!(
            "code=apple-code&state={state}&user={}",
            apple_user_form_value("太郎", "佐藤")
        ),
    )
    .await;
    assert_eq!(response.status(), StatusCode::SEE_OTHER);
    assert_eq!(location_header(&response), "/");
    let session = session_cookie(&response);
    let me = send(&app, get("/api/v1/auth/me", Some(&session))).await;
    assert_eq!(
        json_body(me).await["username"],
        "newuser@privaterelay.appleid.com"
    );
    assert_eq!(
        display_name_of(&pool, "newuser@privaterelay.appleid.com")
            .await
            .as_deref(),
        Some("佐藤 太郎")
    );

    let token_request = mock
        .token_requests
        .lock()
        .expect("mutex lock should not be poisoned")[0]
        .clone();
    assert_eq!(token_request["code"], "apple-code");
    assert_eq!(token_request["client_id"], APPLE_SERVICE_ID);
    assert_eq!(
        token_request["redirect_uri"],
        format!("http://localhost:5173{}", apple_login::CALLBACK_PATH)
    );
    let secret = apple_client_secret_claims(&token_request);
    assert_eq!(secret["sub"], APPLE_SERVICE_ID);
    assert_eq!(secret["iss"], "TEAM123456");

    let sealed = identity_tokens(&pool, LoginProvider::Apple, "A1")
        .await
        .refresh_token
        .expect("refresh token should be kept");
    assert!(!sealed.contains(APPLE_REFRESH_TOKEN));
}

/// `state` が合わない・ID トークンの `nonce` がこのログインのものでない・メールアドレスが
/// 確かめられていない、のどれでもログインさせない。
#[sqlx::test]
async fn apple_callback_rejects_what_does_not_belong_to_this_login(pool: SqlitePool) {
    let mock = start_apple_mock(apple_claims("A1", "user@example.com")).await;
    let app = test_app_with_apple_login(pool.clone(), &mock).await;

    let (_, _, cookie) = start_apple_web_login(&app, &mock).await;
    let response = apple_callback(&app, &cookie, "code=c&state=forged").await;
    assert_eq!(location_header(&response), "/login?oauthError=apple_failed");
    assert!(
        mock.token_requests
            .lock()
            .expect("mutex lock should not be poisoned")
            .is_empty()
    );

    let (state, _, cookie) = start_apple_web_login(&app, &mock).await;
    mock.claims
        .lock()
        .expect("mutex lock should not be poisoned")["nonce"] = json!("other");
    let response = apple_callback(&app, &cookie, &format!("code=c&state={state}")).await;
    assert_eq!(location_header(&response), "/login?oauthError=apple_failed");

    let (state, _, cookie) = start_apple_web_login(&app, &mock).await;
    mock.claims
        .lock()
        .expect("mutex lock should not be poisoned")["email_verified"] = json!(false);
    let response = apple_callback(&app, &cookie, &format!("code=c&state={state}")).await;
    assert_eq!(location_header(&response), "/login?oauthError=apple_failed");

    assert_eq!(count_users(&pool).await, 0);
}

async fn app_apple_login_response(
    app: &Router,
    nonce: &str,
    name: Option<(&str, &str)>,
) -> Response {
    let mut body = json!({ "authorizationCode": "app-code", "nonce": nonce });
    if let Some((given, family)) = name {
        body["givenName"] = json!(given);
        body["familyName"] = json!(family);
    }
    send(
        app,
        app_request(Method::POST, "/api/v1/app/auth/apple", None, Some(body)),
    )
    .await
}

/// アプリの認可コードは Bundle ID 宛てに、redirect_uri を付けずに引き換える。
#[sqlx::test]
async fn app_apple_login_exchanges_the_code_for_the_bundle_id(pool: SqlitePool) {
    let mock = start_apple_mock(apple_claims("A1", "newuser@example.com")).await;
    let app = test_app_with_apple_login(pool.clone(), &mock).await;
    let nonce = start_apple_app_login(&app, &mock).await;

    let response = app_apple_login_response(&app, &nonce, Some(("花子", "山田"))).await;
    assert_eq!(response.status(), StatusCode::OK);
    let body = json_body(response).await;
    let token = body["token"].as_str().expect("token should be a string");
    assert_eq!(app_me_status(&app, token).await, StatusCode::OK);
    assert_eq!(
        display_name_of(&pool, "newuser@example.com")
            .await
            .as_deref(),
        Some("山田 花子")
    );

    let token_request = mock
        .token_requests
        .lock()
        .expect("mutex lock should not be poisoned")[0]
        .clone();
    assert_eq!(token_request["client_id"], APPLE_BUNDLE_ID);
    assert!(!token_request.contains_key("redirect_uri"));
    assert_eq!(
        apple_client_secret_claims(&token_request)["sub"],
        APPLE_BUNDLE_ID
    );

    // 同じ nonce はもう使えない。
    let response = app_apple_login_response(&app, &nonce, None).await;
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[sqlx::test]
async fn app_apple_login_is_unavailable_when_apple_login_is_disabled(pool: SqlitePool) {
    let app = test_app(pool).await;
    let nonce = app_login_nonce(&app).await;
    let response = app_apple_login_response(&app, &nonce, None).await;
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
}

/// Apple の連携を解除すると、リフレッシュトークンを、発行した側 (アプリ) の `client_id` で取り消す。
#[sqlx::test]
async fn unlink_revokes_the_apple_token_with_the_client_that_issued_it(pool: SqlitePool) {
    insert_user(&pool, "kyoko@example.com", "password").await;
    let mock = start_apple_mock(apple_claims("A1", "kyoko@example.com")).await;
    let app = test_app_with_apple_login(pool.clone(), &mock).await;
    let nonce = start_apple_app_login(&app, &mock).await;
    let body = json_body(app_apple_login_response(&app, &nonce, None).await).await;
    let token = body["token"].as_str().expect("token should be a string");

    let response = send(
        &app,
        app_request(
            Method::DELETE,
            "/api/v1/account/identities/apple",
            Some(token),
            None,
        ),
    )
    .await;
    assert_eq!(response.status(), StatusCode::NO_CONTENT);

    let revoke_requests = mock
        .revoke_requests
        .lock()
        .expect("mutex lock should not be poisoned")
        .clone();
    assert_eq!(revoke_requests.len(), 1);
    assert_eq!(revoke_requests[0]["client_id"], APPLE_BUNDLE_ID);
    assert_eq!(revoke_requests[0]["token"], APPLE_REFRESH_TOKEN);
    assert_eq!(revoke_requests[0]["token_type_hint"], "refresh_token");
    assert_eq!(
        apple_client_secret_claims(&revoke_requests[0])["sub"],
        APPLE_BUNDLE_ID
    );
}

/// 猶予期間を過ぎたアカウントを消す前に、Apple のトークンも取り消す。
#[sqlx::test]
async fn purge_revokes_the_apple_token_before_deleting(pool: SqlitePool) {
    let mock = start_apple_mock(apple_claims("A1", "leaving@example.com")).await;
    let app = test_app_with_apple_login(pool.clone(), &mock).await;
    let (state, _, cookie) = start_apple_web_login(&app, &mock).await;
    let response = apple_callback(&app, &cookie, &format!("code=c&state={state}")).await;
    assert_eq!(location_header(&response), "/");
    set_deletion_scheduled(
        &pool,
        "leaving@example.com",
        "2000-01-01T00:00:00.000Z",
        "user",
    )
    .await;

    let deleted = bp_carnet::account::purge_expired(
        &pool,
        &LineLoginClient::disabled(),
        &apple_client(&mock),
    )
    .await
    .expect("物理削除できるはず");

    assert_eq!(deleted, 1);
    let revoke_requests = mock
        .revoke_requests
        .lock()
        .expect("mutex lock should not be poisoned")
        .clone();
    assert_eq!(revoke_requests.len(), 1);
    assert_eq!(revoke_requests[0]["client_id"], APPLE_SERVICE_ID);
    assert_eq!(revoke_requests[0]["token"], APPLE_REFRESH_TOKEN);
}

/// ウェブとアプリの両方でログインした人は、リフレッシュトークンで取り消せなければ (期限切れ等)、
/// アプリで受け取ったアクセストークンで取り消す。ウェブでログインし直しても、アクセストークンは残る。
#[sqlx::test]
async fn purge_falls_back_to_the_app_access_token(pool: SqlitePool) {
    let mock = start_line_mock(line_id_token("U1", Some("leaving@example.com"))).await;
    let app = test_app_with_line_login(pool.clone(), &mock).await;
    let nonce = app_login_nonce(&app).await;
    assert_eq!(
        app_line_login_response(&app, &nonce).await.status(),
        StatusCode::OK
    );
    run_login_flow(&app, LoginProvider::Line).await;
    assert!(
        identity_tokens(&pool, LoginProvider::Line, "U1")
            .await
            .refresh_token
            .is_some()
    );
    assert!(
        identity_tokens(&pool, LoginProvider::Line, "U1")
            .await
            .access_token
            .is_some()
    );
    set_deletion_scheduled(
        &pool,
        "leaving@example.com",
        "2000-01-01T00:00:00.000Z",
        "user",
    )
    .await;
    *mock
        .refresh_status
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
    let token_requests = mock
        .token_requests
        .lock()
        .expect("mutex lock should not be poisoned")
        .clone();
    assert_eq!(
        token_requests.last().expect("refresh should be tried")["grant_type"],
        "refresh_token"
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

/// アプリからの申し込みでは、リンクに `app=1` を付けて、リンクの先 (ブラウザ) で
/// アプリに戻るよう案内させる。登録済みなら、ブラウザのログイン画面へのリンクを送らない。
#[sqlx::test]
async fn mail_links_requested_from_the_app_are_marked(pool: SqlitePool) {
    insert_user(&pool, "kyoko@example.com", "password").await;
    let (app, sent) = test_app_with_mail(pool).await;

    for (path, email) in [
        ("/api/v1/auth/signup", "new@example.com"),
        ("/api/v1/auth/signup", "kyoko@example.com"),
        ("/api/v1/auth/password-reset", "kyoko@example.com"),
    ] {
        let response = send(
            &app,
            app_request(Method::POST, path, None, Some(json!({ "email": email }))),
        )
        .await;
        assert_eq!(response.status(), StatusCode::NO_CONTENT, "{path}");
    }

    let mails = wait_for_mails(&sent, 3).await;
    let body_of = |needle: &str| {
        mails
            .iter()
            .find(|mail| mail.body.contains(needle))
            .unwrap_or_else(|| panic!("{needle} のメールが届くはず: {mails:?}"))
            .body
            .clone()
    };
    let verify = body_of("/verify-email?token=");
    assert!(verify.contains("&app=1"), "{verify}");
    let reset = body_of("/reset-password?token=");
    assert!(reset.contains("&app=1"), "{reset}");
    let registered = body_of("既に登録されています");
    assert!(registered.contains("アプリに戻って"), "{registered}");
    assert!(!registered.contains("/login"), "{registered}");
}
