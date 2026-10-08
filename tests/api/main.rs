//! `/api/v1/*` を HTTP レベルで通しで検証する統合テスト。
//! ルーティング・セッション Cookie の発行/検証・エラー envelope まで含めて確認する。
//! 機能ごとのファイルに分け、2つ以上のファイルで使う補助だけをここに置く。何をこの層で確かめるかは AGENTS.md「テストの層」。

// AGENTS.md「コードの規約」: テストは .unwrap() ではなく .expect("失敗理由") を使う。
// ファイル全体がテストのため cfg_attr は不要 (src/lib.rs と対比)。
#![warn(clippy::unwrap_used)]

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use axum::Json;
use axum::Router;
use axum::body::Body;
use axum::extract::{Form, Path, Query, State};
use axum::http::{Method, Request, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{get as get_route, post as post_route};
use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use bp_carnet::apple_login::{self, AppleLoginClient};
use bp_carnet::auth;
use bp_carnet::config::{Config, OcrCosts, OcrPricing};
use bp_carnet::google_login::GoogleLoginClient;
use bp_carnet::line_login::{self, LineLoginClient};
use bp_carnet::mail::{LINK_HELP, Mailer, SentMail};
use bp_carnet::ocr::OcrService;
use bp_carnet::payments::app_store::{self, AppStoreClient};
use bp_carnet::payments::stripe::StripeClient;
use bp_carnet::{Services, build_app};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use sqlx::SqlitePool;
use tower::ServiceExt;
use tower_sessions::cookie::Key;

mod admin;
mod app;
mod apple;
mod deletion;
mod email_change;
mod email_link;
mod export;
mod google;
mod guards;
mod in_app_purchase;
mod line;
mod login;
mod ocr;
mod password;
mod proxy;
mod records;
mod settings;
mod site;
mod stripe;
mod summary;
mod unlink;

/// 本番で必ず入れる設定 (`Config::check_required`) を埋めた設定。メールのリンクは `disabled_services` の
/// 記録する `Mailer` と同じ URL にそろえる。
fn test_config() -> Config {
    let mut config = Config::default();
    config.server.public_url = "http://localhost:3010".to_owned();
    config.server.contact_url = "https://example.com/contact/".to_owned();
    config.mail.host = "127.0.0.1".to_owned();
    config.mail.from = "BP Carnet <noreply@example.com>".to_owned();
    config
}

/// OCR・Google・LINE・Apple のログインはいずれも無効化した状態を既定にする (実行環境の `GEMINI_API_KEY`・
/// `GOOGLE_OAUTH_CLIENT_ID`/`_SECRET`・`LINE_LOGIN_CHANNEL_ID`/`_SECRET`・`APPLE_*` に依存させないため)。
/// 有効時の挙動を確認するテストは `test_app_with_ocr_enabled`/`test_app_with_google_login`/
/// `test_app_with_line_login`/`test_app_with_apple_login` を使う。
async fn test_app(pool: SqlitePool) -> Router {
    test_app_with_config(pool, &test_config()).await
}

/// `[password]` 等の設定を差し替えた状態のアプリ。
async fn test_app_with_config(pool: SqlitePool, config: &Config) -> Router {
    build_test_app(pool, config, disabled_services()).await
}

/// テストで使う、買い足し1回で付ける量 (1/1000円)。売値で割り切れる値にして、売値に直した額を読みやすくする。
const TEST_TOPUP_GRANT_MILLI_YEN: i64 = 60_000;

/// テストで使う無料枠・単価・買い足しの量。無料枠は無制限にし、上限を確かめるテストは
/// ユーザーごとの上限 (`set_ocr_budget`) で入れる。
fn test_ocr_costs() -> OcrCosts {
    OcrCosts {
        free_budget_yen: -1,
        pricing: OcrPricing {
            input_usd_per_million_tokens: 1.0,
            output_usd_per_million_tokens: 4.0,
            jpy_per_usd: 100.0,
        },
        topup_grant_milli_yen: TEST_TOPUP_GRANT_MILLI_YEN,
    }
}

/// 外部サービスをすべて無効にした部品 (メールは送らずに捨てる)。有効にしたい部品だけを構造体更新記法で差し替える。
fn disabled_services() -> Services {
    Services {
        ocr: OcrService::disabled(),
        google_login: GoogleLoginClient::disabled(),
        line_login: Arc::new(LineLoginClient::disabled()),
        apple_login: Arc::new(AppleLoginClient::disabled()),
        mailer: std::sync::Arc::new(Mailer::recording("http://localhost:3010").0),
        stripe: StripeClient::disabled(),
        app_store: AppStoreClient::disabled(),
        ocr_costs: test_ocr_costs(),
    }
}

/// 部品を差し替えてアプリを組み立てる。各 `test_app_*` はこれを通す。
async fn build_test_app(pool: SqlitePool, config: &Config, services: Services) -> Router {
    build_app(pool, Key::generate(), config, services)
        .await
        .expect("failed to build app")
}

/// ダミーの API キーで OCR を有効化した状態のアプリ。Gemini を実際には呼ばない検証
/// (未認証・画像サイズ超過・MIME タイプ不正など、Gemini 呼び出し前に弾かれるもの) 専用。
async fn test_app_with_ocr_enabled(pool: SqlitePool) -> Router {
    let services = Services {
        ocr: OcrService::with_api_key("dummy-key-for-tests"),
        ..disabled_services()
    };
    build_test_app(pool, &test_config(), services).await
}

/// `mock` (`start_google_mock`) の URL で Google ログインを有効化した状態のアプリ。
async fn test_app_with_google_login(pool: SqlitePool, mock: &GoogleMock) -> Router {
    test_app_with_google_login_and_config(pool, mock, &test_config()).await
}

/// Google ログインを有効化し、設定も差し替えた状態のアプリ。
async fn test_app_with_google_login_and_config(
    pool: SqlitePool,
    mock: &GoogleMock,
    config: &Config,
) -> Router {
    let google_login = GoogleLoginClient::for_test(
        "test-client-id",
        "test-client-secret",
        format!(
            "http://localhost:5173{}",
            bp_carnet::google_login::CALLBACK_PATH
        ),
        bp_carnet::google_login::Endpoints {
            token: format!("{}/token", mock.base_url),
            ..Default::default()
        },
    );
    let services = Services {
        google_login,
        ..disabled_services()
    };
    build_test_app(pool, config, services).await
}

/// Google の `/token` エンドポイントを模すローカルサーバー (→ `serve_mock`)。
struct GoogleMock {
    base_url: String,
    /// `/token` へのリクエストボディ (フォームデコード済み)。PKCE の `code_verifier` を
    /// テスト側から検証するために捕捉する。
    captured_token_request: Arc<Mutex<Option<HashMap<String, String>>>>,
}

/// `/token` が返す ID トークンのクレームを指定してモックサーバーを起動する。
async fn start_google_mock(claims: Value) -> GoogleMock {
    start_google_mock_with_sequence(vec![claims]).await
}

/// `responses` を ID トークンのクレームとして呼び出し順に1つずつ返す (最後に達したら最後の値を
/// 返し続ける) `/token` モック。同じ `sub` に異なる `email` を返す、別々の `sub` に同じ `email` を返す等、
/// 呼び出しごとに違う Google アカウントを返したいテストで使う。
async fn start_google_mock_with_sequence(responses: Vec<Value>) -> GoogleMock {
    let captured_token_request = Arc::new(Mutex::new(None));
    let captured_for_handler = captured_token_request.clone();
    let next_index = Arc::new(std::sync::atomic::AtomicUsize::new(0));

    let app = Router::new()
        .route(
            "/token",
        post_route(move |Form(body): Form<HashMap<String, String>>| {
            let captured = captured_for_handler.clone();
            let responses = responses.clone();
            let next_index = next_index.clone();
            async move {
                *captured.lock().expect("mutex lock should not be poisoned") = Some(body);
                let i = next_index.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                let claims = responses
                    .get(i)
                    .or_else(|| responses.last())
                    .expect("responses should not be empty");
                axum::Json(json!({ "access_token": "mock-access-token", "id_token": unsigned_jwt(claims) }))
            }
        }),
    );

    GoogleMock {
        base_url: serve_mock(app).await,
        captured_token_request,
    }
}

/// 外部サービスを模す `app` をローカルの空きポートで立て、その URL を返す。
/// `wiremock` 等の dev-dependency を増やさず、既存依存 (axum・tokio) だけで済ませる。
async fn serve_mock(app: Router) -> String {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("failed to bind mock listener");
    let addr = listener.local_addr().expect("failed to read mock addr");
    tokio::spawn(async move {
        axum::serve(listener, app)
            .await
            .expect("mock server failed");
    });
    format!("http://{addr}")
}

/// Google Workspace (`hd` あり) のアカウントのクレーム。Google が持ち主を保証するメールとして受け付けられる。
fn google_claims(sub: &str, email: &str, email_verified: bool) -> Value {
    json!({ "sub": sub, "email": email, "email_verified": email_verified, "hd": "example.com" })
}

/// 署名の無い JWT・JWS。サーバーは ID トークンや App Store の取引・通知の署名を確かめない
/// (`src/jwt.rs` の `unverified_payload`)。
fn unsigned_jwt(claims: &Value) -> String {
    format!(
        "{}.{}.",
        URL_SAFE_NO_PAD.encode(r#"{"alg":"none"}"#),
        URL_SAFE_NO_PAD.encode(claims.to_string())
    )
}

/// 管理者が `--create-user` で作ったユーザー (メール確認済み) を作る。
async fn insert_user(pool: &SqlitePool, username: &str, password: &str) {
    insert_user_with_role(pool, username, password, auth::Role::User).await;
}

/// `--create-user --admin` で作った管理者を作る。
async fn insert_admin(pool: &SqlitePool, username: &str, password: &str) {
    insert_user_with_role(pool, username, password, auth::Role::Admin).await;
}

async fn insert_user_with_role(
    pool: &SqlitePool,
    username: &str,
    password: &str,
    role: auth::Role,
) {
    let password_hash = auth::hash_password(password).expect("failed to hash password");
    auth::create_user(
        pool,
        username,
        auth::NewPassword::Usable(&password_hash),
        role,
    )
    .await
    .expect("failed to insert test user");
}

/// 監査ログの記録 (`admin_audit_log`) を古い順に `(admin_user_id, target_user_id, action)` で返す。
async fn audit_log_entries(pool: &SqlitePool) -> Vec<(i64, i64, String)> {
    sqlx::query!(
        r#"SELECT admin_user_id AS "admin_user_id!: i64", target_user_id AS "target_user_id!: i64",
                  action FROM admin_audit_log ORDER BY id"#
    )
    .fetch_all(pool)
    .await
    .expect("failed to read admin_audit_log")
    .into_iter()
    .map(|row| (row.admin_user_id, row.target_user_id, row.action))
    .collect()
}

async fn user_id_of(pool: &SqlitePool, username: &str) -> i64 {
    sqlx::query_scalar!(
        r#"SELECT id AS "id!: i64" FROM users WHERE username = ?"#,
        username
    )
    .fetch_one(pool)
    .await
    .expect("failed to look up user id")
}

async fn is_frozen(pool: &SqlitePool, username: &str) -> bool {
    sqlx::query_scalar!("SELECT frozen FROM users WHERE username = ?", username)
        .fetch_one(pool)
        .await
        .expect("failed to read frozen")
        != 0
}

/// 凍結を直接書き込む。
async fn freeze_user(pool: &SqlitePool, username: &str) {
    sqlx::query!("UPDATE users SET frozen = 1 WHERE username = ?", username)
        .execute(pool)
        .await
        .expect("テストデータを更新できるはず");
}

/// 削除予約 (予定日時と起点) を直接書き込む。
async fn set_deletion_scheduled(
    pool: &SqlitePool,
    username: &str,
    scheduled_at: &str,
    origin: &str,
) {
    sqlx::query!(
        "UPDATE users SET deletion_scheduled_at = ?, deletion_origin = ? WHERE username = ?",
        scheduled_at,
        origin,
        username
    )
    .execute(pool)
    .await
    .expect("テストデータを更新できるはず");
}

/// 削除予定日時。`None` なら予約なし。
async fn deletion_scheduled_at_of(pool: &SqlitePool, username: &str) -> Option<String> {
    sqlx::query_scalar!(
        r#"SELECT deletion_scheduled_at AS "deletion_scheduled_at: String" FROM users WHERE username = ?"#,
        username
    )
    .fetch_one(pool)
    .await
    .expect("failed to read deletion_scheduled_at")
}

async fn count_users(pool: &SqlitePool) -> i64 {
    sqlx::query_scalar!(r#"SELECT COUNT(*) AS "count!: i64" FROM users"#)
        .fetch_one(pool)
        .await
        .expect("failed to count users")
}

async fn session_generation_of(pool: &SqlitePool, username: &str) -> i64 {
    sqlx::query_scalar!(
        r#"SELECT session_generation AS "session_generation!: i64" FROM users WHERE username = ?"#,
        username
    )
    .fetch_one(pool)
    .await
    .expect("failed to look up session generation")
}

/// `provider` の外部のアカウントの識別の数。`subject` を渡すとそれだけを数える。
async fn count_identities(
    pool: &SqlitePool,
    provider: LoginProvider,
    subject: Option<&str>,
) -> i64 {
    let provider = provider.path();
    sqlx::query_scalar!(
        r#"SELECT COUNT(*) AS "count!: i64" FROM oauth_identities
           WHERE provider = ? AND (? IS NULL OR subject = ?)"#,
        provider,
        subject,
        subject
    )
    .fetch_one(pool)
    .await
    .expect("failed to count identities")
}

/// 外部のアカウントの識別に保存した、連携の取り消しに使うトークン (暗号化したもの)。
struct IdentityTokens {
    refresh_token: Option<String>,
    access_token: Option<String>,
}

async fn identity_tokens(
    pool: &SqlitePool,
    provider: LoginProvider,
    subject: &str,
) -> IdentityTokens {
    let provider = provider.path();
    let row = sqlx::query!(
        r#"SELECT refresh_token AS "refresh_token: String", access_token AS "access_token: String"
           FROM oauth_identities WHERE provider = ? AND subject = ?"#,
        provider,
        subject
    )
    .fetch_one(pool)
    .await
    .expect("failed to read the identity's tokens");
    IdentityTokens {
        refresh_token: row.refresh_token,
        access_token: row.access_token,
    }
}

/// `app` に1件送って応答を返す。
async fn send(app: &Router, request: Request<Body>) -> Response {
    app.clone()
        .oneshot(request)
        .await
        .expect("oneshot request should not fail")
}

/// リクエストを組み立てる。`body` が `Some` なら JSON として送る。
fn request(
    method: Method,
    uri: &str,
    cookie: Option<&str>,
    extra_headers: &[(header::HeaderName, &str)],
    body: Option<Value>,
) -> Request<Body> {
    let mut builder = Request::builder().method(method).uri(uri);
    if let Some(cookie) = cookie {
        builder = builder.header(header::COOKIE, cookie);
    }
    for (name, value) in extra_headers {
        builder = builder.header(name.clone(), *value);
    }
    let body = match body {
        Some(body) => {
            builder = builder.header(header::CONTENT_TYPE, "application/json");
            Body::from(body.to_string())
        }
        None => Body::empty(),
    };
    builder.body(body).expect("failed to build request")
}

fn get(uri: &str, cookie: Option<&str>) -> Request<Body> {
    request(Method::GET, uri, cookie, &[], None)
}

fn post_json(uri: &str, cookie: Option<&str>, body: Value) -> Request<Body> {
    request(Method::POST, uri, cookie, &[], Some(body))
}

fn put_json(uri: &str, cookie: Option<&str>, body: Value) -> Request<Body> {
    request(Method::PUT, uri, cookie, &[], Some(body))
}

fn delete(uri: &str, cookie: Option<&str>) -> Request<Body> {
    request(Method::DELETE, uri, cookie, &[], None)
}

/// ログインしてセッション Cookie を得るヘルパー。血圧記録のテストのように、
/// 認証済みの状態から始めたいテストの前準備として使う。
async fn login_and_get_cookie(app: &Router, username: &str, password: &str) -> String {
    let response = send(app, login_request(username, password)).await;
    assert_eq!(response.status(), StatusCode::OK);
    session_cookie(&response)
}

/// 一般ユーザー `admin` / `password` を作ってログインした状態の、既定の構成のアプリとセッション Cookie。
async fn logged_in_app(pool: SqlitePool) -> (Router, String) {
    insert_user(&pool, "admin", "password").await;
    let app = test_app(pool).await;
    let cookie = login_and_get_cookie(&app, "admin", "password").await;
    (app, cookie)
}

fn login_request(username: &str, password: &str) -> Request<Body> {
    post_json(
        "/api/v1/auth/login",
        None,
        json!({"username": username, "password": password}),
    )
}

/// `admin` / `password` でログインするリクエストにヘッダーを足す。CSRF チェックやプロキシのヘッダーの扱いを確かめるテストで使う。
fn login_request_with_headers(headers: &[(header::HeaderName, &str)]) -> Request<Body> {
    request(
        Method::POST,
        "/api/v1/auth/login",
        None,
        headers,
        Some(json!({"username": "admin", "password": "password"})),
    )
}

/// 応答が `status` で、エラー envelope の `code` が `code` であることを確かめる。
async fn assert_error(response: Response, status: StatusCode, code: &str) {
    assert_error_case(response, status, code, "").await;
}

/// `assert_error` に、場合の表を回すテストで、どの場合で落ちたかを出す文言 `label` を足したもの。
async fn assert_error_case(response: Response, status: StatusCode, code: &str, label: &str) {
    assert_eq!(response.status(), status, "{label}");
    assert_eq!(json_body(response).await["error"]["code"], code, "{label}");
}

async fn json_body(response: Response) -> Value {
    let bytes = body_bytes(response).await;
    serde_json::from_slice(&bytes).expect("response body should be JSON")
}

/// `GET /auth/providers` の応答の本文 (200 であることも確かめる)。
async fn auth_providers(app: Router) -> Value {
    let response = send(&app, get("/api/v1/auth/providers", None)).await;
    assert_eq!(response.status(), StatusCode::OK);
    json_body(response).await
}

/// エクスポートCSVをレコード列 (ヘッダー名 → 値) として読む。BOM を取り除いてから
/// csv クレートでパースするので、メモに改行やカンマが入っていても行が崩れない。
async fn export_csv_rows(response: Response) -> Vec<std::collections::HashMap<String, String>> {
    let bytes = body_bytes(response).await;
    assert_eq!(
        &bytes[..3],
        [0xEF, 0xBB, 0xBF],
        "CSV should start with a UTF-8 BOM so Excel opens it correctly"
    );
    csv::Reader::from_reader(&bytes[3..])
        .deserialize()
        .map(|row| row.expect("CSV row should be readable"))
        .collect()
}

/// CSV エクスポート等、JSON ではない生のレスポンスボディを読むためのヘルパー。
async fn body_bytes(response: Response) -> Vec<u8> {
    response
        .into_body()
        .collect()
        .await
        .expect("failed to read body")
        .to_bytes()
        .to_vec()
}

/// レスポンスの `Set-Cookie` からセッション Cookie を取り出す (`secure_cookie` が無効のときの
/// セッション Cookie 名は `"id"`、→ `bp_carnet::session::cookie_name`)。
fn session_cookie(response: &Response) -> String {
    cookie_pair(response, "id")
}

fn create_record_request(cookie: &str, body: Value) -> Request<Body> {
    post_json("/api/v1/records", Some(cookie), body)
}

fn valid_record_body() -> Value {
    json!({
        "localMeasuredAt": "2026-09-07T08:30",
        "systolic": 120,
        "diastolic": 80,
        "pulse": 65,
        "memo": "朝食前"
    })
}

fn record_uri(id: i64) -> String {
    format!("/api/v1/records/{id}")
}

/// 記録を1件登録し、その id を返すヘルパー。編集・削除テストの前準備として使う。
async fn create_record(app: &Router, cookie: &str, body: Value) -> i64 {
    let response = send(app, create_record_request(cookie, body)).await;
    assert_eq!(response.status(), StatusCode::CREATED);
    json_body(response).await["id"]
        .as_i64()
        .expect("created record should have an id")
}

fn set_cookie_values(response: &Response) -> Vec<String> {
    response
        .headers()
        .get_all(header::SET_COOKIE)
        .iter()
        .map(|v| {
            v.to_str()
                .expect("cookie header should be valid utf-8")
                .to_string()
        })
        .collect()
}

fn find_set_cookie<'a>(cookies: &'a [String], name: &str) -> Option<&'a String> {
    cookies.iter().find(|c| c.starts_with(&format!("{name}=")))
}

/// `Set-Cookie` から `name` の Cookie を、次のリクエストの `Cookie` ヘッダーにそのまま使える
/// `name=value` の形で取り出す。Google OAuth コールバックのようにセッションと `oauth_state` の
/// Cookie が同時に発行される場合があるため、最初の1件ではなく名前で選び出す。
fn cookie_pair(response: &Response, name: &str) -> String {
    let cookies = set_cookie_values(response);
    find_set_cookie(&cookies, name)
        .unwrap_or_else(|| panic!("{name} cookie should be set"))
        .split(';')
        .next()
        .expect("cookie header should contain a name=value pair")
        .to_string()
}

fn location_header(response: &Response) -> String {
    response
        .headers()
        .get(header::LOCATION)
        .expect("response should have a Location header")
        .to_str()
        .expect("Location header should be valid utf-8")
        .to_string()
}

fn query_param(url: &str, key: &str) -> String {
    reqwest::Url::parse(url)
        .expect("url should parse")
        .query_pairs()
        .find(|(k, _)| k == key)
        .map(|(_, v)| v.into_owned())
        .unwrap_or_else(|| panic!("query param {key} not found in {url}"))
}

/// ログインのプロバイダー。URL の `/auth/{path}/login`・`callback` と、`state` を持つ Cookie の名前。
#[derive(Clone, Copy)]
enum LoginProvider {
    Google,
    Line,
    Apple,
}

impl LoginProvider {
    fn path(self) -> &'static str {
        match self {
            Self::Google => "google",
            Self::Line => "line",
            Self::Apple => "apple",
        }
    }

    fn state_cookie(self) -> &'static str {
        match self {
            Self::Google => "oauth_state",
            Self::Line => "line_oauth_state",
            Self::Apple => "apple_oauth_state",
        }
    }
}

/// login → callback を一気通貫で叩き、コールバックの `Response` を返す。
async fn run_login_flow(app: &Router, provider: LoginProvider) -> Response {
    let path = provider.path();
    let login_response = send(app, get(&format!("/api/v1/auth/{path}/login"), None)).await;
    let location = location_header(&login_response);
    let state = query_param(&location, "state");
    let state_cookie = cookie_pair(&login_response, provider.state_cookie());

    send(
        app,
        get(
            &format!("/api/v1/auth/{path}/callback?code=test-code&state={state}"),
            Some(&state_cookie),
        ),
    )
    .await
}

/// 接続元アドレスを付与する。`axum::serve` が `ConnectInfo` として入れるものを、
/// `oneshot` で駆動するテストでは手で入れる。
fn from_peer(mut request: Request<Body>, addr: &str) -> Request<Body> {
    let addr: std::net::SocketAddr = addr.parse().expect("接続元アドレスが不正");
    request
        .extensions_mut()
        .insert(axum::extract::ConnectInfo(addr));
    request
}

/// 本人によるパスワード変更 (docs/authentication.md) のテスト。
fn change_password_request(cookie: Option<&str>, current: &str, new: &str) -> Request<Body> {
    put_json(
        "/api/v1/account/password",
        cookie,
        json!({"currentPassword": current, "newPassword": new}),
    )
}

/// パスワードポリシーを既定より厳しくした設定 (8文字以上・数字必須)。
fn strict_password_config() -> Config {
    toml::from_str::<Config>("[password]\nmin_length = 8\nrequired_classes = [\"digit\"]\n")
        .expect("設定を読めるはず")
}

/// 利用者のアカウント情報・血圧記録の閲覧 (docs/authentication.md) のテスト。
async fn insert_record_at(pool: &SqlitePool, username: &str, measured_at: &str, systolic: i64) {
    let user_id = user_id_of(pool, username).await;
    sqlx::query!(
        "INSERT INTO bp_records (user_id, measured_at, systolic, diastolic) VALUES (?, ?, ?, 80)",
        user_id,
        measured_at,
        systolic
    )
    .execute(pool)
    .await
    .expect("failed to insert test record");
}

/// 凍結・解除 (docs/authentication.md) のテスト。
fn set_frozen_request(cookie: &str, target_id: i64, frozen: bool) -> Request<Body> {
    put_json(
        &format!("/api/v1/admin/users/{target_id}/frozen"),
        Some(cookie),
        json!({ "frozen": frozen }),
    )
}

/// 管理画面からのユーザー作成 (docs/authentication.md) のテスト。
fn create_user_request(cookie: &str, body: Value) -> Request<Body> {
    post_json("/api/v1/admin/users", Some(cookie), body)
}

/// 管理者によるパスワード再設定 (docs/authentication.md) のテスト。
fn reset_password_request(cookie: &str, target_id: i64, password: &str) -> Request<Body> {
    put_json(
        &format!("/api/v1/admin/users/{target_id}/password"),
        Some(cookie),
        json!({ "password": password }),
    )
}

/// 管理者によるユーザー削除 (docs/authentication.md) のテスト。
fn admin_delete_request(cookie: &str, target_id: i64) -> Request<Body> {
    post_json(
        &format!("/api/v1/admin/users/{target_id}/deletion"),
        Some(cookie),
        json!({}),
    )
}

fn admin_cancel_deletion_request(cookie: &str, target_id: i64) -> Request<Body> {
    delete(
        &format!("/api/v1/admin/users/{target_id}/deletion"),
        Some(cookie),
    )
}

/// 送ったメールを記録する状態のアプリ。
async fn test_app_with_mail(pool: SqlitePool) -> (Router, Arc<Mutex<Vec<SentMail>>>) {
    test_app_with_mail_and_config(pool, &test_config()).await
}

/// 送ったメールを記録し、設定も差し替えた状態のアプリ。
async fn test_app_with_mail_and_config(
    pool: SqlitePool,
    config: &Config,
) -> (Router, Arc<Mutex<Vec<SentMail>>>) {
    let (mailer, sent) = Mailer::recording("http://localhost:3010");
    let services = Services {
        mailer: std::sync::Arc::new(mailer),
        ..disabled_services()
    };
    let app = build_test_app(pool, config, services).await;
    (app, sent)
}

fn sent_mails(sent: &Arc<Mutex<Vec<SentMail>>>) -> Vec<SentMail> {
    sent.lock()
        .expect("mutex lock should not be poisoned")
        .clone()
}

/// メールの本文に載った `path` のリンク (`/verify-email` 等) から、トークンを取り出す。
fn link_token(mail: &SentMail, path: &str) -> String {
    let (_, rest) = mail
        .body
        .split_once(&format!("{path}?token="))
        .expect("リンクが載っているはず");
    rest.split(|c: char| c.is_whitespace() || c == '&')
        .next()
        .expect("トークンが続くはず")
        .to_string()
}

fn password_reset_request(email: &str) -> Request<Body> {
    post_json(
        "/api/v1/auth/password-reset",
        None,
        json!({ "email": email }),
    )
}

fn complete_password_reset_request(token: &str, password: &str) -> Request<Body> {
    post_json(
        "/api/v1/auth/password-reset/complete",
        None,
        json!({ "token": token, "password": password }),
    )
}

/// 応答を待たずに送るメールを、`poll` が値を返すまで待つ。
/// 上限は、CPU が埋まった CI でも届くだけの長さにする (失敗するときにしか効かない)。
async fn wait_for_sent<T>(
    sent: &Arc<Mutex<Vec<SentMail>>>,
    poll: impl Fn(Vec<SentMail>) -> Option<T>,
) -> Option<T> {
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(10);
    loop {
        if let Some(found) = poll(sent_mails(sent)) {
            return Some(found);
        }
        if tokio::time::Instant::now() >= deadline {
            return None;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
}

/// 応答を待たずに送るメールを、`count` 通届くまで待って取り出す。
async fn wait_for_mails(sent: &Arc<Mutex<Vec<SentMail>>>, count: usize) -> Vec<SentMail> {
    wait_for_sent(sent, |mails| (mails.len() >= count).then_some(mails))
        .await
        .unwrap_or_else(|| panic!("{count} 通のメールが届くはず"))
}

/// 応答を待たずに送るメールが、少し待っても `count` 通から増えないことを確かめる。
async fn assert_mail_count_settles(sent: &Arc<Mutex<Vec<SentMail>>>, count: usize) {
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    assert_eq!(sent_mails(sent).len(), count);
}

async fn terms_agreement(pool: &SqlitePool, username: &str) -> (Option<String>, Option<String>) {
    let row = sqlx::query!(
        "SELECT terms_version, terms_agreed_at FROM users WHERE username = ?",
        username
    )
    .fetch_one(pool)
    .await
    .expect("user should exist");
    (row.terms_version, row.terms_agreed_at)
}

/// LINE の `/token`・`/verify`・`/profile`・`/v3/token`・`/deauthorize` を模すローカルサーバー。
/// Google のモック (`GoogleMock`) と同じく、既存依存だけでその場限りに立てる。
struct LineMock {
    base_url: String,
    /// `/verify` が返す ID トークンのペイロード。
    id_token: Arc<Mutex<Value>>,
    /// `/profile` が返すユーザーID (アクセストークンの持ち主)。既定は ID トークンの `sub`。
    profile_user_id: Arc<Mutex<String>>,
    /// リフレッシュトークンでの `/token` が返すステータス (期限切れを模す)。
    refresh_status: Arc<Mutex<StatusCode>>,
    /// `/deauthorize` が返すステータス。
    deauthorize_status: Arc<Mutex<StatusCode>>,
    /// `/token` へのリクエスト (フォーム) を呼ばれた順に。
    token_requests: Arc<Mutex<Vec<HashMap<String, String>>>>,
    verify_requests: Arc<Mutex<Vec<HashMap<String, String>>>>,
    /// `/profile` へのリクエストの `Authorization` ヘッダー。
    profile_requests: Arc<Mutex<Vec<String>>>,
    channel_token_requests: Arc<Mutex<Vec<HashMap<String, String>>>>,
    /// `/deauthorize` へのリクエストの `Authorization` ヘッダーと本文。
    deauthorize_requests: Arc<Mutex<Vec<(String, Value)>>>,
}

/// 認可コードと引き換えに返すリフレッシュトークン。
const LINE_REFRESH_TOKEN: &str = "line-refresh-token";

async fn start_line_mock(id_token: Value) -> LineMock {
    let mock = LineMock {
        base_url: String::new(),
        profile_user_id: Arc::new(Mutex::new(
            id_token["sub"].as_str().unwrap_or_default().to_string(),
        )),
        id_token: Arc::new(Mutex::new(id_token)),
        refresh_status: Arc::new(Mutex::new(StatusCode::OK)),
        deauthorize_status: Arc::new(Mutex::new(StatusCode::NO_CONTENT)),
        token_requests: Arc::default(),
        verify_requests: Arc::default(),
        profile_requests: Arc::default(),
        channel_token_requests: Arc::default(),
        deauthorize_requests: Arc::default(),
    };

    let token_requests = mock.token_requests.clone();
    let refresh_status = mock.refresh_status.clone();
    let verify_requests = mock.verify_requests.clone();
    let id_token = mock.id_token.clone();
    let profile_requests = mock.profile_requests.clone();
    let profile_user_id = mock.profile_user_id.clone();
    let channel_token_requests = mock.channel_token_requests.clone();
    let deauthorize_requests = mock.deauthorize_requests.clone();
    let deauthorize_status = mock.deauthorize_status.clone();
    let app = Router::new()
        .route(
            "/token",
            post_route(move |Form(body): Form<HashMap<String, String>>| {
                let token_requests = token_requests.clone();
                let refresh_status = refresh_status.clone();
                async move {
                    let response = if body.get("grant_type").map(String::as_str) == Some("refresh_token") {
                        let status = *refresh_status.lock().expect("mutex lock should not be poisoned");
                        if status != StatusCode::OK {
                            token_requests.lock().expect("mutex lock should not be poisoned").push(body);
                            return (status, axum::Json(json!({ "error": "invalid_grant" })));
                        }
                        json!({ "access_token": "refreshed-access-token", "refresh_token": body["refresh_token"], "expires_in": 2592000 })
                    } else {
                        json!({
                            "access_token": "line-access-token",
                            "refresh_token": LINE_REFRESH_TOKEN,
                            "id_token": "line-id-token",
                            "expires_in": 2592000
                        })
                    };
                    token_requests.lock().expect("mutex lock should not be poisoned").push(body);
                    (StatusCode::OK, axum::Json(response))
                }
            }),
        )
        .route(
            "/verify",
            post_route(move |Form(body): Form<HashMap<String, String>>| {
                let verify_requests = verify_requests.clone();
                let id_token = id_token.clone();
                async move {
                    verify_requests.lock().expect("mutex lock should not be poisoned").push(body);
                    axum::Json(id_token.lock().expect("mutex lock should not be poisoned").clone())
                }
            }),
        )
        .route(
            "/profile",
            get_route(move |headers: axum::http::HeaderMap| {
                let profile_requests = profile_requests.clone();
                let profile_user_id = profile_user_id.clone();
                async move {
                    let authorization = headers
                        .get(header::AUTHORIZATION)
                        .and_then(|value| value.to_str().ok())
                        .unwrap_or_default()
                        .to_string();
                    profile_requests.lock().expect("mutex lock should not be poisoned").push(authorization);
                    let user_id = profile_user_id.lock().expect("mutex lock should not be poisoned").clone();
                    axum::Json(json!({ "userId": user_id, "displayName": "LINE の名前" }))
                }
            }),
        )
        .route(
            "/v3/token",
            post_route(move |Form(body): Form<HashMap<String, String>>| {
                let channel_token_requests = channel_token_requests.clone();
                async move {
                    channel_token_requests.lock().expect("mutex lock should not be poisoned").push(body);
                    axum::Json(json!({ "token_type": "Bearer", "access_token": "channel-access-token", "expires_in": 900 }))
                }
            }),
        )
        .route(
            "/deauthorize",
            post_route(move |headers: axum::http::HeaderMap, axum::Json(body): axum::Json<Value>| {
                let deauthorize_requests = deauthorize_requests.clone();
                let deauthorize_status = deauthorize_status.clone();
                async move {
                    let authorization = headers
                        .get(header::AUTHORIZATION)
                        .and_then(|value| value.to_str().ok())
                        .unwrap_or_default()
                        .to_string();
                    deauthorize_requests
                        .lock()
                        .expect("mutex lock should not be poisoned")
                        .push((authorization, body));
                    *deauthorize_status.lock().expect("mutex lock should not be poisoned")
                }
            }),
        );

    LineMock {
        base_url: serve_mock(app).await,
        ..mock
    }
}

fn line_id_token(sub: &str, email: Option<&str>) -> Value {
    match email {
        Some(email) => {
            json!({ "iss": "https://access.line.me", "sub": sub, "aud": "test-channel-id", "email": email })
        }
        None => json!({ "iss": "https://access.line.me", "sub": sub, "aud": "test-channel-id" }),
    }
}

fn line_client(mock: &LineMock) -> LineLoginClient {
    LineLoginClient::for_test(
        "test-channel-id",
        "test-channel-secret",
        format!("http://localhost:5173{}", line_login::CALLBACK_PATH),
        line_login::Endpoints {
            authorize: "https://access.line.me/oauth2/v2.1/authorize".to_string(),
            token: format!("{}/token", mock.base_url),
            verify: format!("{}/verify", mock.base_url),
            profile: format!("{}/profile", mock.base_url),
            channel_token: format!("{}/v3/token", mock.base_url),
            deauthorize: format!("{}/deauthorize", mock.base_url),
        },
    )
}

async fn test_app_with_line_login(pool: SqlitePool, mock: &LineMock) -> Router {
    let services = Services {
        line_login: Arc::new(line_client(mock)),
        ..disabled_services()
    };
    build_test_app(pool, &test_config(), services).await
}

/// 全ユーザーの無料の枠の上限 (円) と、使った累計 (1/1000円) を書き換える。
async fn set_ocr_budget(pool: &SqlitePool, budget_yen: i64, spent_milli_yen: i64) {
    sqlx::query!(
        "UPDATE users SET ocr_budget_yen = ?, ocr_spent_milli_yen = ?",
        budget_yen,
        spent_milli_yen
    )
    .execute(pool)
    .await
    .expect("failed to set the budget");
}

/// 買い足した枠の残りの合計 (1/1000円)。
async fn paid_remaining(pool: &SqlitePool, user_id: i64) -> i64 {
    bp_carnet::payments::ocr_quota::available(pool, user_id)
        .await
        .expect("grants should be readable")
        .iter()
        .map(|grant| grant.remaining_milli_yen)
        .sum()
}

const APP_ORIGIN: &str = "capacitor://localhost";

/// アプリの WebView と同じく、`Origin: capacitor://localhost` を付けたリクエスト。
fn app_request(
    method: Method,
    uri: &str,
    token: Option<&str>,
    body: Option<Value>,
) -> Request<Body> {
    let authorization = token.map(|token| format!("Bearer {token}"));
    let mut headers = vec![(header::ORIGIN, APP_ORIGIN), (header::HOST, "bp.example")];
    if let Some(authorization) = &authorization {
        headers.push((header::AUTHORIZATION, authorization.as_str()));
    }
    request(method, uri, None, &headers, body)
}

async fn app_me_status(app: &Router, token: &str) -> StatusCode {
    send(
        app,
        app_request(Method::GET, "/api/v1/auth/me", Some(token), None),
    )
    .await
    .status()
}

/// アプリの LINE ログインで LINE SDK が返すアクセストークン。
const LINE_APP_ACCESS_TOKEN: &str = "app-access-token";

async fn app_login_nonce(app: &Router) -> String {
    let response = send(
        app,
        app_request(Method::POST, "/api/v1/app/auth/nonce", None, None),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    json_body(response).await["nonce"]
        .as_str()
        .expect("nonce should be a string")
        .to_string()
}

async fn app_line_login_response(app: &Router, nonce: &str) -> Response<Body> {
    send(
        app,
        app_request(
            Method::POST,
            "/api/v1/app/auth/line",
            None,
            Some(json!({
                "idToken": "sdk-id-token",
                "accessToken": LINE_APP_ACCESS_TOKEN,
                "nonce": nonce,
            })),
        ),
    )
    .await
}

/// テストのためだけに作った鍵 (Apple には登録していない)。
const APPLE_TEST_PRIVATE_KEY: &str = "-----BEGIN PRIVATE KEY-----
MIGHAgEAMBMGByqGSM49AgEGCCqGSM49AwEHBG0wawIBAQQgxxiPfi/vjhWHeioS
mETQ4ZhL0FsiWicWx83WNb1HJZmhRANCAATC/ECNudnDQqIEF3MsxKUHdgOyKcUk
meRzhDnSIYMONHYscf1pwLzCmbxcVSREfbcX7AN1Mb+SNerfKhE6qh/a
-----END PRIVATE KEY-----
";

const APPLE_BUNDLE_ID: &str = "com.example.bp";

/// 購入の行の、買い手が国内か。`column` は決済の ID の列名。
async fn grant_domestic(pool: &SqlitePool, column: &str, id: &str) -> Option<i64> {
    sqlx::query_scalar::<_, Option<i64>>(&format!(
        "SELECT domestic FROM ocr_quota_grants WHERE {column} = ?"
    ))
    .bind(id)
    .fetch_one(pool)
    .await
    .expect("grant should exist")
}

async fn ocr_budget_yen_of(pool: &SqlitePool, username: &str) -> Option<i64> {
    sqlx::query_scalar::<_, Option<i64>>("SELECT ocr_budget_yen FROM users WHERE username = ?")
        .bind(username)
        .fetch_one(pool)
        .await
        .expect("failed to read ocr_budget_yen")
}
