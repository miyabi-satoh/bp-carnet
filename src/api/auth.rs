//! ログイン・ログアウト・ログイン中ユーザー取得・Google・LINE・Apple でのログイン。

use axum::Json;
use axum::extract::rejection::{FormRejection, QueryRejection};
use axum::extract::{Form, Query, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Redirect, Response};
use serde::{Deserialize, Serialize};
use tower_sessions::Session;
use utoipa::ToSchema;
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;

use crate::account::{self, DeletionOrigin};
use crate::apple_login::Client as AppleClient;
use crate::auth::{self, AuthUser, Role};
use crate::config::CharClass;
use crate::error::{AppError, AppJson, ErrorResponse};
use crate::external_login;
use crate::forwarded::{self, ClientIp};
use crate::google_login;
use crate::oauth_cookie;
use crate::oauth_identity::{self, ExternalAccount, FindOrCreateError, Provider, RevocationTokens};
use crate::settings;
use crate::state::AppState;
use crate::terms;

#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct LoginRequest {
    #[schema(example = "admin")]
    username: String,
    #[schema(example = "password")]
    password: String,
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UserResponse {
    pub(super) id: i64,
    pub(super) username: String,
    /// 猶予期間中だったアカウント削除を、このログインで取り消したか (docs/authentication.md)。
    /// 取り消しは専用の操作ではなくログインの副作用のため、利用者に知らせる必要がある。
    pub(super) deletion_cancelled: bool,
}

/// ログイン中ユーザー。ログイン応答 (`UserResponse`) と違い、画面の表示に要る個人設定も返す。
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct MeResponse {
    id: i64,
    username: String,
    /// 個人設定のタイムゾーン (IANA 名)。frontend は「今日」「現在時刻」をこのタイムゾーンで求める。
    #[schema(example = "Asia/Tokyo")]
    timezone: String,
    /// タイムゾーンが自動設定か。`true` なら、frontend はブラウザのタイムゾーンが `timezone` と違うときに
    /// `PUT /settings/detected-timezone` で揃える。
    timezone_auto: bool,
    /// 表示名。Google ログインで取得したプロフィールの名前 (無ければ `null`)。
    display_name: Option<String>,
    /// プロフィール画像の URL。Google ログインで取得したものだけ (無ければ `null`)。
    avatar_url: Option<String>,
    /// 権限。frontend は管理者向けの導線の表示可否をこれで判断する。
    role: Role,
    /// ID/PW ログインに使えるパスワードを持つか (`users.password_usable`)。`false` (Google 専用
    /// アカウント) では、設定画面に「パスワードを変更」の行を出さない。
    password_usable: bool,
    /// 連携している外部アカウント。設定画面の「ログイン方法」に並べる。
    linked_providers: Vec<Provider>,
    /// 削除予定日時 (UTC、RFC3339)。予約が無ければ `null`。設定画面に予定日を出す (docs/authentication.md)。
    #[schema(example = "2026-10-12T00:00:00.000Z")]
    deletion_scheduled_at: Option<String>,
    /// 削除予約の起点。予約が無ければ `null`。本人が起点なら、ログインし直すと取り消せると案内する。
    deletion_origin: Option<DeletionOrigin>,
}

#[utoipa::path(
    post,
    path = "/auth/login",
    request_body = LoginRequest,
    responses(
        (status = 200, description = "ログイン成功", body = UserResponse),
        (status = 400, description = "リクエストボディが不正 (JSON として解釈できない等)", body = ErrorResponse),
        (status = 401, description = "ユーザー名またはパスワードが正しくない", body = ErrorResponse),
        (status = 403, description = "アカウントが凍結されている", body = ErrorResponse),
        (status = 415, description = "Content-Type が application/json でない", body = ErrorResponse),
        (status = 422, description = "リクエストボディに必須フィールドが無い等", body = ErrorResponse),
        (status = 429, description = "ログイン試行回数の上限を超えた", body = ErrorResponse),
    )
)]
async fn login(
    session: Session,
    State(state): State<AppState>,
    ClientIp(client_ip): ClientIp,
    AppJson(payload): AppJson<LoginRequest>,
) -> Result<Json<UserResponse>, AppError> {
    let user = authenticate_password(&state, client_ip, &payload).await?;
    auth::establish_session(&session, &state.pool, user.id).await?;
    Ok(Json(user))
}

/// ID/パスワードを照合し、ログインしてよい利用者を返す。ウェブ (セッション) とアプリ (トークン、
/// docs/mobile-app.md) のログインで共通。ログインの証しを渡すのは呼び出し側。
pub(super) async fn authenticate_password(
    state: &AppState,
    client_ip: Option<std::net::IpAddr>,
    payload: &LoginRequest,
) -> Result<UserResponse, AppError> {
    // 接続元ごとに数える (docs/authentication.md)。ユーザー名だけで数えると、第三者が本人の名前で
    // 試し続けるだけで別の端末の本人を締め出せる。接続元が分からない構成ではユーザー名単位に戻る。
    // 接続元は `rate_limit_key` で数える (IPv6 は /64 ごと)。接続元を前に置くのは、キーの長さの
    // 切り詰めで接続元が落ちないようにするため。
    let client_key = client_ip.map(forwarded::rate_limit_key);
    let attempt_key = match &client_key {
        Some(client_key) => format!("{client_key} {}", payload.username),
        None => payload.username.clone(),
    };
    let reservation = state
        .login_rate_limiter
        .try_acquire(&attempt_key)
        .ok_or(AppError::TooManyRequests)?;
    let client_reservation = match client_key {
        Some(client_key) => {
            let Some(client_reservation) = state.login_client_rate_limiter.try_acquire(&client_key)
            else {
                // 照合まで進まないので、先に取った枠は数えない。
                state.login_rate_limiter.release(&attempt_key, reservation);
                return Err(AppError::TooManyRequests);
            };
            Some((client_key, client_reservation))
        }
        None => None,
    };

    let user = auth::authenticate(&state.pool, &payload.username, &payload.password)
        .await?
        .ok_or(AppError::InvalidCredentials)?;

    // パスワードが正しければ、凍結の有無に関わらず try_acquire で消費した分を解放する。
    // 凍結中の利用者が再試行するたびに上限へ近づくと、凍結中であることを伝える前に
    // 429 になってしまう。
    state.login_rate_limiter.release(&attempt_key, reservation);
    if let Some((client_key, client_reservation)) = client_reservation {
        state
            .login_client_rate_limiter
            .release(&client_key, client_reservation);
    }

    // 猶予期間中のログインは、本人が起点の削除予約の取り消しを兼ねる (本人向けの取り消し UI は
    // 持たない。管理者が起点の予約は残す → `account::DeletionOrigin`)。凍結の判定より前に置く:
    // 本人が取り消せるのはこの経路だけなので、後に回すと凍結された利用者は自分で申し込んだ
    // 削除を止められなくなる。
    let deletion_cancelled = account::cancel_user_deletion(&state.pool, user.id).await?;

    // 凍結の判定はパスワードの照合の後に置く。先に見ると、凍結中かどうかがユーザー名だけで
    // 分かってしまう。
    if user.frozen {
        return Err(AppError::AccountFrozen);
    }

    Ok(UserResponse {
        id: user.id,
        username: user.username,
        deletion_cancelled,
    })
}

/// ログイン済みかどうかに関わらず、Cookie が指すセッションを破棄する。
/// 未ログイン状態での呼び出しはエラーにせず単に成功として扱う。
#[utoipa::path(
    post,
    path = "/auth/logout",
    responses((status = 204, description = "ログアウト成功")),
)]
async fn logout(session: Session) -> Result<StatusCode, AppError> {
    session.flush().await.map_err(auth::Error::Session)?;
    Ok(StatusCode::NO_CONTENT)
}

/// ログイン中のユーザー情報を返す。フロントエンドは起動時にこれを呼び、
/// 401 が返れば未ログインとしてログイン画面に遷移する。
///
/// DB 上のユーザー存在確認は `AuthUser` エクストラクタ側で行っている
/// (セッションは有効だが削除済みユーザーの場合も 401 になる)。
#[utoipa::path(
    get,
    path = "/auth/me",
    responses(
        (status = 200, description = "ログイン中", body = MeResponse),
        (status = 401, description = "未ログイン", body = ErrorResponse),
        (status = 500, description = "保存されているタイムゾーンが tzdb に存在しない", body = ErrorResponse),
    )
)]
async fn me(user: AuthUser, State(state): State<AppState>) -> Result<Json<MeResponse>, AppError> {
    let settings = settings::load_required(&state.pool, user.id).await?;
    // frontend はこのタイムゾーンで「今日」を求めるため、使えない名前は画面に渡さずここで止める。
    settings.user_timezone()?;
    let profile = auth::load_profile(&state.pool, user.id).await?;
    let deletion = account::pending_deletion(&state.pool, user.id).await?;
    Ok(Json(MeResponse {
        id: user.id,
        username: user.username,
        timezone: settings.timezone,
        timezone_auto: settings.timezone_auto,
        display_name: profile.display_name,
        avatar_url: profile.avatar_url,
        role: user.role,
        password_usable: profile.password_usable,
        linked_providers: oauth_identity::linked_providers(&state.pool, user.id).await?,
        deletion_scheduled_at: deletion.as_ref().map(|d| d.scheduled_at.clone()),
        deletion_origin: deletion.as_ref().map(|d| d.origin),
    }))
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct AuthProvidersResponse {
    /// `false` の場合、フロントエンドは「Googleでログイン」を隠す
    /// (`GET /ocr/status` の `enabled` と同じ思想)。
    google_enabled: bool,
    /// `false` の場合、フロントエンドは「LINEでログイン」を隠す。
    line_enabled: bool,
    /// `false` の場合、フロントエンドは「Appleでサインイン」を隠す。
    apple_enabled: bool,
    /// アプリで Apple のログイン (`/app/auth/apple`) を受け付けるか。アプリの中ではこちらで
    /// 「Appleでサインイン」の表示を決める (ウェブと要る設定が違うため。docs/mobile-app.md)。
    apple_app_enabled: bool,
    /// 利用規約・プライバシーポリシーの版 (docs/data-model.md)。
    #[schema(example = "1")]
    terms_version: &'static str,
    /// `true` の場合、フロントエンドは「オープンβテスト中」のバッジと一文を出す
    /// (`[server] beta_notice`、docs/architecture.md)。
    beta_notice: bool,
    /// 問い合わせ先の URL。フロントエンドは「運営者にお問い合わせください」の文言にリンクを添える
    /// (`[server] contact_url`、docs/architecture.md)。
    #[schema(example = "https://example.com/contact/")]
    contact_url: String,
    /// 紹介ページの URL。空でなければ、ウェブのログイン画面に紹介へのリンクを出す
    /// (`[server] intro_url`、docs/architecture.md)。
    #[schema(example = "https://example.com/app/")]
    intro_url: String,
}

/// 未認証で叩ける公開エンドポイント。ログイン画面はこれを見て「Googleでログイン」「LINEでログイン」の
/// 表示可否を判断する (`/auth/me` は未ログイン時に 401 を返すため、この判断には使えない)。
#[utoipa::path(
    get,
    path = "/auth/providers",
    responses((status = 200, description = "利用可能な追加のログイン方式", body = AuthProvidersResponse)),
)]
async fn providers(State(state): State<AppState>) -> Json<AuthProvidersResponse> {
    Json(AuthProvidersResponse {
        google_enabled: state.google_login.enabled(),
        line_enabled: state.line_login.enabled(),
        apple_enabled: state.apple_login.enabled(),
        apple_app_enabled: state.apple_login.app_enabled(),
        terms_version: terms::VERSION,
        beta_notice: state.beta_notice,
        contact_url: state.contact_url.to_string(),
        intro_url: state.intro_url.to_string(),
    })
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct PasswordPolicyResponse {
    /// 最小の文字数。
    #[schema(example = 4)]
    min_length: usize,
    /// 少なくとも1文字ずつ含める必要がある文字種。空なら文字種は問わない。
    required_classes: Vec<CharClass>,
}

/// パスワードの最小強度 (`[password]` 設定) を返す。パスワードを入力する画面が、条件を
/// あらかじめ示し、送信前に同じ規則で確認するために使う (規則の判定自体はサーバーが行う)。
///
/// 未認証で叩ける: パスワードを設定する画面はサインアップ・再設定のように未ログインからも
/// 開くため。最小文字数が分かっても総当たりの助けにはならない。
#[utoipa::path(
    get,
    path = "/auth/password-policy",
    responses((status = 200, description = "パスワードの最小強度", body = PasswordPolicyResponse)),
)]
async fn password_policy(State(state): State<AppState>) -> Json<PasswordPolicyResponse> {
    Json(PasswordPolicyResponse {
        min_length: state.password.min_length,
        required_classes: state.password.required_classes.clone(),
    })
}

/// `oauthError` クエリパラメータ付きでログイン画面へ戻すための URL を作る。
fn login_error_url(code: &str) -> String {
    format!("/login?oauthError={code}")
}

/// ログイン画面に返す `oauthError` のコード。画面の文言がプロバイダーの名前を含むため、プロバイダーごとに分ける。
struct CallbackErrors {
    failed: &'static str,
    disabled: &'static str,
    account_conflict: &'static str,
}

impl CallbackErrors {
    fn of(provider: Provider) -> Self {
        match provider {
            Provider::Google => Self {
                failed: "google_failed",
                disabled: "google_disabled",
                account_conflict: "google_account_conflict",
            },
            Provider::Line => Self {
                failed: "line_failed",
                disabled: "line_disabled",
                account_conflict: "line_account_conflict",
            },
            Provider::Apple => Self {
                failed: "apple_failed",
                disabled: "apple_disabled",
                account_conflict: "apple_account_conflict",
            },
        }
    }
}

/// 凍結中のアカウントへのログインを弾いたときのコード。ID/PW ログインと同じ文言にするため、プロバイダーで分けない。
const ACCOUNT_FROZEN: &str = "account_frozen";

/// LINE の同意画面でメールアドレスの提供をオフにされたときのコード (docs/authentication.md)。
const LINE_EMAIL_REQUIRED: &str = "line_email_required";
/// Google のメールアドレスを、Google が持ち主として保証していない (`GoogleAccountError::EmailUntrusted`)。
const GOOGLE_EMAIL_UNTRUSTED: &str = "google_email_untrusted";

/// Google の認可エンドポイントへリダイレクトする。CSRF (state) 対策・PKCE 用の専用 Cookie
/// (`crate::oauth_cookie`) を発行する。無効時は JSON ではなくログイン画面へのリダイレクトに
/// する (ブラウザのフルナビゲーションでのみ叩かれる想定のため)。
#[utoipa::path(
    get,
    path = "/auth/google/login",
    responses((status = 303, description = "Google の認可画面、または /login (無効時・エラー時) へのリダイレクト")),
)]
async fn google_login(State(state): State<AppState>) -> Response {
    let errors = CallbackErrors::of(Provider::Google);
    if !state.google_login.enabled() {
        return Redirect::to(&login_error_url(errors.disabled)).into_response();
    }

    let oauth_state = google_login::generate_state();
    let code_verifier = external_login::generate_code_verifier();
    let code_challenge = external_login::code_challenge_s256(&code_verifier);

    let Some(auth_url) = state
        .google_login
        .authorization_url(&oauth_state, &code_challenge)
    else {
        // enabled() を確認済みなので通常は到達しない。念のための安全側の分岐。
        return Redirect::to(&login_error_url(errors.failed)).into_response();
    };

    redirect_with_state_cookie(
        &auth_url,
        Provider::Google,
        &oauth_cookie::pack(&[&oauth_state, &code_verifier]),
        state.secure_cookie,
    )
}

/// LINE の認可エンドポイントへリダイレクトする。Google と同じく state・PKCE の専用 Cookie を
/// 発行し、ID トークンを結び付ける `nonce` もそこに入れる。
#[utoipa::path(
    get,
    path = "/auth/line/login",
    responses((status = 303, description = "LINE の認可画面、または /login (無効時・エラー時) へのリダイレクト")),
)]
async fn line_login(State(state): State<AppState>) -> Response {
    let errors = CallbackErrors::of(Provider::Line);
    if !state.line_login.enabled() {
        return Redirect::to(&login_error_url(errors.disabled)).into_response();
    }

    let oauth_state = external_login::generate_state();
    let code_verifier = external_login::generate_code_verifier();
    let code_challenge = external_login::code_challenge_s256(&code_verifier);
    let nonce = external_login::generate_nonce();

    let Some(auth_url) = state
        .line_login
        .authorization_url(&oauth_state, &code_challenge, &nonce)
    else {
        // enabled() を確認済みなので通常は到達しない。
        return Redirect::to(&login_error_url(errors.failed)).into_response();
    };

    redirect_with_state_cookie(
        &auth_url,
        Provider::Line,
        &oauth_cookie::pack(&[&oauth_state, &code_verifier, &nonce]),
        state.secure_cookie,
    )
}

/// Apple の認可エンドポイントへリダイレクトする。`state` と `nonce` を専用 Cookie に入れる
/// (Apple は PKCE を文書にしていないので、`code_verifier` は無い。docs/authentication.md)。
#[utoipa::path(
    get,
    path = "/auth/apple/login",
    responses((status = 303, description = "Apple の認可画面、または /login (無効時・エラー時) へのリダイレクト")),
)]
async fn apple_login(State(state): State<AppState>) -> Response {
    let errors = CallbackErrors::of(Provider::Apple);
    if !state.apple_login.enabled() {
        return Redirect::to(&login_error_url(errors.disabled)).into_response();
    }

    let oauth_state = external_login::generate_state();
    let nonce = external_login::generate_nonce();
    let Some(auth_url) = state.apple_login.authorization_url(&oauth_state, &nonce) else {
        // enabled() を確認済みなので通常は到達しない。
        return Redirect::to(&login_error_url(errors.failed)).into_response();
    };

    redirect_with_state_cookie(
        &auth_url,
        Provider::Apple,
        &oauth_cookie::pack(&[&oauth_state, &nonce]),
        state.secure_cookie,
    )
}

fn redirect_with_state_cookie(
    auth_url: &str,
    provider: Provider,
    cookie_value: &str,
    secure_cookie: bool,
) -> Response {
    let mut response = Redirect::to(auth_url).into_response();
    response.headers_mut().append(
        header::SET_COOKIE,
        oauth_cookie::set_header(provider, cookie_value, secure_cookie),
    );
    response
}

/// コールバックで受け取る値 (Google・LINE はクエリ、Apple はフォーム)。
///
/// ADR: 抽出には (他のハンドラと違い) `AppQuery` などではなく生の `Query`・`Form` を使う。
/// `App*` の抽出失敗は常に JSON envelope の `AppError` になるが、コールバックは失敗時も
/// `/login?oauthError=...` へのリダイレクトで返す必要があるため。
#[derive(Debug, Deserialize, ToSchema)]
struct CallbackParams {
    #[serde(default)]
    code: Option<String>,
    #[serde(default)]
    state: Option<String>,
    /// ユーザーが同意画面でキャンセルした場合等に付く (例: Google の `access_denied`、
    /// LINE の `ACCESS_DENIED`)。値の中身は使わず、有無だけを見る。
    #[serde(default)]
    error: Option<String>,
}

/// コールバックの `code` を取り出し、`state` を Cookie のものと照らし合わせる。
/// Cookie に詰めた `N` 個の値 (先頭が `state`) と `code` を返す。
fn verify_callback<const N: usize>(
    params: Result<Query<CallbackParams>, QueryRejection>,
    headers: &HeaderMap,
    provider: Provider,
    secure_cookie: bool,
) -> Option<(String, [String; N])> {
    let Query(params) = params.ok()?;
    verify_state(params, headers, provider, secure_cookie)
}

/// [`verify_callback`] の、受け取り方 (クエリ・フォーム) によらない部分。
fn verify_state<const N: usize>(
    params: CallbackParams,
    headers: &HeaderMap,
    provider: Provider,
    secure_cookie: bool,
) -> Option<(String, [String; N])> {
    if params.error.is_some() {
        return None;
    }
    let code = params.code?;
    let query_state = params.state?;

    let cookie_value = oauth_cookie::read(headers, provider, secure_cookie)?;
    let values = oauth_cookie::unpack::<N>(&cookie_value)?;
    if values.first() != Some(&query_state.as_str()) {
        return None;
    }
    Some((code, values.map(str::to_string)))
}

/// コールバックの結果をリダイレクトにする。state/PKCE 専用 Cookie は成否に関わらず必ず破棄する (使い捨て)。
fn callback_response(
    outcome: Result<bool, &'static str>,
    provider: Provider,
    secure_cookie: bool,
) -> Response {
    let mut response = match outcome {
        // ID/PW ログインの `deletionCancelled` に相当する通知。リダイレクトで返すため
        // レスポンスボディを持てず、クエリパラメータで渡す (`oauthError` と同じ方式)。
        Ok(true) => Redirect::to("/?deletionCancelled=1").into_response(),
        Ok(false) => Redirect::to("/").into_response(),
        Err(code) => Redirect::to(&login_error_url(code)).into_response(),
    };
    response.headers_mut().append(
        header::SET_COOKIE,
        oauth_cookie::clear_header(provider, secure_cookie),
    );
    response
}

/// Google からのコールバック。成功・失敗いずれの経路でも必ずリダイレクトで終わる
/// (JSON 応答は無い)。
#[utoipa::path(
    get,
    path = "/auth/google/callback",
    responses((status = 303, description = "ログイン成功 (/) 、または失敗 (/login?oauthError=...) へのリダイレクト")),
)]
async fn google_callback(
    State(state): State<AppState>,
    session: Session,
    headers: HeaderMap,
    params: Result<Query<CallbackParams>, QueryRejection>,
) -> Response {
    let outcome = handle_google_callback(&state, &session, &headers, params).await;
    callback_response(outcome, Provider::Google, state.secure_cookie)
}

/// `google_callback` 本体。`&'static str` はフロントエンドに渡す `oauthError` コード。
/// 成功時の `bool` は、このログインで削除予約を取り消したかどうか。
async fn handle_google_callback(
    state: &AppState,
    session: &Session,
    headers: &HeaderMap,
    params: Result<Query<CallbackParams>, QueryRejection>,
) -> Result<bool, &'static str> {
    let errors = CallbackErrors::of(Provider::Google);
    if !state.google_login.enabled() {
        return Err(errors.disabled);
    }
    let (code, [_, code_verifier]) =
        verify_callback(params, headers, Provider::Google, state.secure_cookie)
            .ok_or(errors.failed)?;

    let account = google_account(state, &code, Some(&code_verifier))
        .await
        .map_err(GoogleAccountError::web_code)?;
    complete_web_login(state, session, &account, &RevocationTokens::default()).await
}

/// `google_account` が断った理由。ウェブはログイン画面へ戻すときのコードに、アプリは `AppError` に換える。
pub(super) enum GoogleAccountError {
    /// Google とのやり取りに失敗した。原因はログに残す。
    Failed,
    /// メールアドレスを Google が持ち主として保証していない (`GoogleIdToken::email_is_trusted`)。
    /// 結び付けも新しいアカウントも作らず、メールアドレスで入るよう案内する
    /// (他人のメールアドレスを登録した Google アカウントから、そのアカウントに入れてしまうため)。
    EmailUntrusted,
}

impl GoogleAccountError {
    fn web_code(self) -> &'static str {
        match self {
            Self::Failed => CallbackErrors::of(Provider::Google).failed,
            Self::EmailUntrusted => GOOGLE_EMAIL_UNTRUSTED,
        }
    }

    pub(super) fn app_error(self) -> AppError {
        match self {
            Self::Failed => AppError::ExternalLoginFailed,
            Self::EmailUntrusted => AppError::GoogleEmailUntrusted,
        }
    }
}

/// Google の認可コードを引き換え、ログインする人のアカウントを得る。ウェブとアプリ (docs/mobile-app.md)
/// で共通。
pub(super) async fn google_account(
    state: &AppState,
    code: &str,
    code_verifier: Option<&str>,
) -> Result<ExternalAccount, GoogleAccountError> {
    let id_token = state
        .google_login
        .exchange_code(code, code_verifier)
        .await
        .map_err(|err| {
            tracing::warn!(error = %crate::error_chain_line(&err), "Google トークン交換に失敗しました");
            GoogleAccountError::Failed
        })?;

    if !id_token.email_is_trusted() {
        return Err(GoogleAccountError::EmailUntrusted);
    }
    Ok(ExternalAccount::from(id_token))
}

/// LINE からのコールバック。Google と同じく、成否に関わらずリダイレクトで終わる。
#[utoipa::path(
    get,
    path = "/auth/line/callback",
    responses((status = 303, description = "ログイン成功 (/) 、または失敗 (/login?oauthError=...) へのリダイレクト")),
)]
async fn line_callback(
    State(state): State<AppState>,
    session: Session,
    headers: HeaderMap,
    params: Result<Query<CallbackParams>, QueryRejection>,
) -> Response {
    let outcome = handle_line_callback(&state, &session, &headers, params).await;
    callback_response(outcome, Provider::Line, state.secure_cookie)
}

/// `line_callback` 本体。戻り値は `handle_google_callback` と同じ。
async fn handle_line_callback(
    state: &AppState,
    session: &Session,
    headers: &HeaderMap,
    params: Result<Query<CallbackParams>, QueryRejection>,
) -> Result<bool, &'static str> {
    let errors = CallbackErrors::of(Provider::Line);
    if !state.line_login.enabled() {
        return Err(errors.disabled);
    }
    let (code, [_, code_verifier, nonce]) =
        verify_callback(params, headers, Provider::Line, state.secure_cookie)
            .ok_or(errors.failed)?;

    let tokens = state
        .line_login
        .exchange_code(&code, &code_verifier)
        .await
        .map_err(|err| {
            tracing::warn!(error = %crate::error_chain_line(&err), "LINE のトークン交換に失敗しました");
            errors.failed
        })?;
    let account = line_account(state, &tokens.id_token, &nonce)
        .await
        .ok_or(errors.failed)?;

    // メールアドレスの提供を断られたら、アカウントを作らずに案内する。ユーザーID = メールアドレスの
    // 作りを崩さないため (docs/authentication.md)。
    if account.email.is_none() {
        return Err(LINE_EMAIL_REQUIRED);
    }

    let revocation_tokens = RevocationTokens {
        refresh_token: state.line_login.seal_token(&tokens.refresh_token),
        access_token: None,
    };
    complete_web_login(state, session, &account, &revocation_tokens).await
}

/// LINE の ID トークンを確かめ、ログインする人のアカウントを得る。ウェブとアプリ (docs/mobile-app.md)
/// で共通。失敗の原因はログに残し、`None` を返す。メールアドレスの有無は呼び出し側が見る
/// (無いときの断り方がウェブとアプリで違うため)。
pub(super) async fn line_account(
    state: &AppState,
    id_token: &str,
    nonce: &str,
) -> Option<ExternalAccount> {
    let id_token = state
        .line_login
        .verify_id_token(id_token, nonce)
        .await
        .inspect_err(|err| {
            tracing::warn!(error = %crate::error_chain_line(err), "LINE の ID トークンの検証に失敗しました");
        })
        .ok()?;
    Some(ExternalAccount::from(id_token))
}

/// Apple からのコールバック (`form_post`)。Apple のサイトからの POST なので、同一オリジンの確認
/// (`crate::csrf`) から外し、`state` の照合で守る (docs/authentication.md)。Google・LINE と同じく、
/// 成否に関わらずリダイレクトで終わる。
#[utoipa::path(
    post,
    path = "/auth/apple/callback",
    request_body(content = AppleCallbackForm, content_type = "application/x-www-form-urlencoded"),
    responses((status = 303, description = "ログイン成功 (/) 、または失敗 (/login?oauthError=...) へのリダイレクト")),
)]
async fn apple_callback(
    State(state): State<AppState>,
    session: Session,
    headers: HeaderMap,
    form: Result<Form<AppleCallbackForm>, FormRejection>,
) -> Response {
    let outcome = handle_apple_callback(&state, &session, &headers, form).await;
    callback_response(outcome, Provider::Apple, state.secure_cookie)
}

#[derive(Debug, Deserialize, ToSchema)]
struct AppleCallbackForm {
    #[serde(flatten)]
    params: CallbackParams,
    /// 最初の承認のときだけ付く、名前とメールアドレスの JSON。
    #[serde(default)]
    user: Option<String>,
}

/// `AppleCallbackForm::user` のうち、使う項目。
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AppleUser {
    #[serde(default)]
    name: AppleUserName,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AppleUserName {
    first_name: Option<String>,
    last_name: Option<String>,
}

/// `apple_callback` 本体。戻り値は `handle_google_callback` と同じ。
async fn handle_apple_callback(
    state: &AppState,
    session: &Session,
    headers: &HeaderMap,
    form: Result<Form<AppleCallbackForm>, FormRejection>,
) -> Result<bool, &'static str> {
    let errors = CallbackErrors::of(Provider::Apple);
    if !state.apple_login.enabled() {
        return Err(errors.disabled);
    }
    let Form(form) = form.map_err(|_| errors.failed)?;
    let (code, [_, nonce]) =
        verify_state(form.params, headers, Provider::Apple, state.secure_cookie)
            .ok_or(errors.failed)?;
    // 名前は読めなくてもログインは止めない (表示名が入らないだけ)。
    let name = form
        .user
        .and_then(|user| serde_json::from_str::<AppleUser>(&user).ok())
        .unwrap_or_default()
        .name;

    let (account, revocation_tokens) = apple_account(
        state,
        AppleClient::Web,
        &code,
        &nonce,
        name.first_name.as_deref(),
        name.last_name.as_deref(),
    )
    .await
    .ok_or(errors.failed)?;
    complete_web_login(state, session, &account, &revocation_tokens).await
}

/// Apple の認可コードを引き換え、ログインする人のアカウントと、連携の取り消しに使うトークンを得る。
/// ウェブとアプリ (docs/mobile-app.md) で共通。失敗の原因はログに残し、`None` を返す。
pub(super) async fn apple_account(
    state: &AppState,
    client: AppleClient,
    code: &str,
    nonce: &str,
    given_name: Option<&str>,
    family_name: Option<&str>,
) -> Option<(ExternalAccount, RevocationTokens)> {
    let tokens = state
        .apple_login
        .exchange_code(client, code, nonce)
        .await
        .inspect_err(|err| {
            tracing::warn!(error = %crate::error_chain_line(err), "Apple の認可コードの引き換えに失敗しました");
        })
        .ok()?;
    let id_token = tokens.id_token;
    if !id_token.email_verified || id_token.email.is_none() {
        tracing::warn!("Apple の ID トークンに、確かめられたメールアドレスがありませんでした");
        return None;
    }
    let account = ExternalAccount::from_apple(id_token, given_name, family_name);
    let revocation_tokens = RevocationTokens {
        refresh_token: tokens
            .refresh_token
            .and_then(|token| state.apple_login.seal_refresh_token(client, &token)),
        access_token: None,
    };
    Some((account, revocation_tokens))
}

/// 外部のアカウントでのログインを受け付けなかった理由。ウェブはログイン画面へ戻すときのコードに、
/// アプリ (docs/mobile-app.md) は `AppError` に換える。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ExternalLoginRejection {
    /// 内部の失敗。原因はログに残してある。
    Failed,
    /// 同じメールアドレスのアカウントに紐付けられない (`FindOrCreateError::UsernameConflict`)。
    AccountConflict,
    Frozen,
}

impl ExternalLoginRejection {
    fn web_code(self, provider: Provider) -> &'static str {
        let errors = CallbackErrors::of(provider);
        match self {
            Self::Failed => errors.failed,
            Self::AccountConflict => errors.account_conflict,
            Self::Frozen => ACCOUNT_FROZEN,
        }
    }

    pub(super) fn app_error(self) -> AppError {
        match self {
            Self::Failed => AppError::ExternalLoginFailed,
            Self::AccountConflict => AppError::ExternalAccountConflict,
            Self::Frozen => AppError::AccountFrozen,
        }
    }
}

/// 外部のアカウントでログインしてよいと決まった利用者。
pub(super) struct ExternalLoginUser {
    pub(super) id: i64,
    /// このログインで、猶予期間中だった削除の予約を取り消したか。
    pub(super) deletion_cancelled: bool,
}

/// Google・LINE・Apple のコールバックの最後の処理。ログインしてよい利用者を決め、セッションを確立する。
async fn complete_web_login(
    state: &AppState,
    session: &Session,
    account: &ExternalAccount,
    revocation_tokens: &RevocationTokens,
) -> Result<bool, &'static str> {
    let user = resolve_external_login(state, account, revocation_tokens)
        .await
        .map_err(|rejection| rejection.web_code(account.provider))?;

    // `AppError` の Display は表に出さない固定文言なので、原因まで辿って記録する
    // (`src/error.rs` の `IntoResponse` と同じ扱い)。
    auth::establish_session(session, &state.pool, user.id)
        .await
        .map_err(|err| {
            tracing::error!(error = %crate::error_chain_line(&err), "セッションの確立に失敗しました");
            CallbackErrors::of(account.provider).failed
        })?;

    Ok(user.deletion_cancelled)
}

/// プロバイダーでの本人確認が済んだ後の、ウェブとアプリ・各提供元に共通の処理。凍結の判定・
/// ユーザーの特定か作成・プロフィールの保存・削除予約の取り消し。ログインの証しを渡すのは呼び出し側。
/// `revocation_tokens` は、連携の取り消しに使うトークン (LINE・Apple)。
pub(super) async fn resolve_external_login(
    state: &AppState,
    account: &ExternalAccount,
    revocation_tokens: &RevocationTokens,
) -> Result<ExternalLoginUser, ExternalLoginRejection> {
    use ExternalLoginRejection::Failed;

    // ID/PW ログインと同じく凍結中は入れない (docs/authentication.md)。判定は
    // `find_or_create_user` より前に置く: 同関数の紐付けは一度きりでやり直せないため、
    // 拒否するログインで `oauth_identities` に恒久的な行を残さない。
    if let Some(frozen_user_id) = oauth_identity::frozen_target(&state.pool, account)
        .await
        .map_err(|err| {
            tracing::error!(error = %crate::error_chain_line(&err), "凍結状態の確認に失敗しました");
            Failed
        })?
    {
        // ID/PW ログインと同じく、本人だと分かった時点で本人が起点の削除予約は取り消す。紐付け済みの
        // アカウントで来たときだけ本人と判断でき、未連携のメールアドレス一致では
        // 判断できない (紐付けを作らずに弾くため)。
        if oauth_identity::find_identity(&state.pool, account.provider, &account.subject)
            .await
            .map_err(|err| {
                tracing::error!(error = %crate::error_chain_line(&err), "外部のアカウントの識別の検索に失敗しました");
                Failed
            })?
            .is_some()
        {
            account::cancel_user_deletion(&state.pool, frozen_user_id)
                .await
                .map_err(|err| {
                    tracing::error!(error = %crate::error_chain_line(&err), "アカウント削除の取り消しに失敗しました");
                    Failed
                })?;
        }
        return Err(ExternalLoginRejection::Frozen);
    }

    let user_id = oauth_identity::find_or_create_user(
        &state.pool,
        account,
        terms::VERSION,
    )
    .await
    .map_err(|err| match err {
        FindOrCreateError::UsernameConflict => ExternalLoginRejection::AccountConflict,
        other => {
            tracing::error!(error = %crate::error_chain_line(&other), "外部のアカウントでのログインのユーザー作成に失敗しました");
            Failed
        }
    })?;

    if !revocation_tokens.is_empty() {
        oauth_identity::save_revocation_tokens(
            &state.pool,
            account.provider,
            &account.subject,
            revocation_tokens,
        )
        .await
        .map_err(|err| {
            tracing::error!(error = %crate::error_chain_line(&err), "連携の取り消しに使うトークンの保存に失敗しました");
            Failed
        })?;
    }

    oauth_identity::save_profile(&state.pool, user_id, account)
        .await
        .map_err(|err| {
            tracing::error!(error = %crate::error_chain_line(&err), "プロフィールの保存に失敗しました");
            Failed
        })?;

    // 猶予期間中のログインは、本人が起点の削除予約の取り消しを兼ねる (ID/PW ログインと同じ)。
    // ログインの証しを渡す前に済ませる: 後に回すと、取り消しに失敗したときに
    // 「ログイン済みなのにエラーを返す」状態になる。
    let deletion_cancelled = account::cancel_user_deletion(&state.pool, user_id)
        .await
        .map_err(|err| {
            tracing::error!(error = %crate::error_chain_line(&err), "アカウント削除の取り消しに失敗しました");
            Failed
        })?;

    Ok(ExternalLoginUser {
        id: user_id,
        deletion_cancelled,
    })
}

pub fn router() -> OpenApiRouter<AppState> {
    // `routes!(a, b, c)` は「同一パスに複数メソッド」をまとめる書き方で、パスの異なるハンドラを
    // 1回の呼び出しに詰めると内部でメソッドが衝突するため、パスごとに `.routes()` を分けて呼ぶ。
    OpenApiRouter::new()
        .routes(routes!(login))
        .routes(routes!(logout))
        .routes(routes!(me))
        .routes(routes!(providers))
        .routes(routes!(password_policy))
        .routes(routes!(google_login))
        .routes(routes!(google_callback))
        .routes(routes!(line_login))
        .routes(routes!(line_callback))
        .routes(routes!(apple_login))
        .routes(routes!(apple_callback))
}
