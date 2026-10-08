//! モバイルアプリのログイン・ログアウト (docs/mobile-app.md)。
//!
//! ウェブのログイン (`super::auth`) と同じ照合を通し、セッションの代わりにトークンを返す。
//! アプリはトークンをキーチェーンにしまい、`Authorization: Bearer` で送る。

use axum::Json;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;

use super::auth::{
    ExternalLoginRejection, GoogleAccountError, LoginRequest, UserResponse, apple_account,
    authenticate_password, google_account, line_account, resolve_external_login,
};
use crate::app_token;
use crate::apple_login::Client as AppleClient;
use crate::error::{AppError, AppJson, ErrorResponse};
use crate::forwarded::{self, ClientIp};
use crate::oauth_identity::{ExternalAccount, RevocationTokens};
use crate::state::AppState;

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct AppLoginResponse {
    /// 以後のリクエストに `Authorization: Bearer` で付けるトークン。
    token: String,
    user: UserResponse,
}

#[utoipa::path(
    post,
    path = "/app/auth/login",
    request_body = LoginRequest,
    responses(
        (status = 200, description = "ログイン成功", body = AppLoginResponse),
        (status = 400, description = "リクエストボディが不正 (JSON として解釈できない等)", body = ErrorResponse),
        (status = 401, description = "ユーザー名またはパスワードが正しくない", body = ErrorResponse),
        (status = 403, description = "アカウントが凍結されている", body = ErrorResponse),
        (status = 415, description = "Content-Type が application/json でない", body = ErrorResponse),
        (status = 422, description = "リクエストボディに必須フィールドが無い等", body = ErrorResponse),
        (status = 429, description = "ログイン試行回数の上限を超えた", body = ErrorResponse),
    )
)]
async fn app_login(
    State(state): State<AppState>,
    ClientIp(client_ip): ClientIp,
    AppJson(payload): AppJson<LoginRequest>,
) -> Result<Json<AppLoginResponse>, AppError> {
    let user = authenticate_password(&state, client_ip, &payload).await?;
    let token = app_token::establish(&state.pool, user.id).await?;
    Ok(Json(AppLoginResponse { token, user }))
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct AppLoginNonceResponse {
    /// LINE・Apple の SDK に渡す値。10分以内に、1回のログインにだけ使える。
    nonce: String,
}

/// LINE・Apple でのログインを始める。アプリは返った `nonce` を SDK に渡し、得た ID トークンと
/// 一緒にログインの入口へ送る (ID トークンの使い回しを防ぐため、docs/mobile-app.md)。
#[utoipa::path(
    post,
    path = "/app/auth/nonce",
    responses(
        (status = 200, description = "発行した nonce", body = AppLoginNonceResponse),
        (status = 429, description = "発行の回数、または発行済みで未使用の数の上限を超えた", body = ErrorResponse),
    )
)]
async fn app_login_nonce(
    State(state): State<AppState>,
    ClientIp(client_ip): ClientIp,
) -> Result<Json<AppLoginNonceResponse>, AppError> {
    state
        .app_nonce_rate_limiter
        .try_acquire(&forwarded::rate_limit_key_or_shared(client_ip))
        .ok_or(AppError::TooManyRequests)?;
    let nonce = state
        .app_login_nonces
        .issue()
        .ok_or(AppError::TooManyRequests)?;
    Ok(Json(AppLoginNonceResponse { nonce }))
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct AppGoogleLoginRequest {
    /// Google Sign-In が offline モードで返すサーバー用の認可コード (`serverAuthCode`)。
    server_auth_code: String,
}

/// Google でログインする。認可コードはサーバーが Google と直接引き換え、ウェブのログインと同じく
/// ID トークンで本人を確かめる (docs/mobile-app.md)。
#[utoipa::path(
    post,
    path = "/app/auth/google",
    request_body = AppGoogleLoginRequest,
    responses(
        (status = 200, description = "ログイン成功", body = AppLoginResponse),
        (status = 400, description = "リクエストボディが不正 (JSON として解釈できない等)", body = ErrorResponse),
        (status = 401, description = "Google で本人を確かめられなかった、またはメールアドレスを Google が保証していない", body = ErrorResponse),
        (status = 403, description = "アカウントが凍結されている", body = ErrorResponse),
        (status = 409, description = "同じメールアドレスのアカウントに紐付けられない", body = ErrorResponse),
        (status = 415, description = "Content-Type が application/json でない", body = ErrorResponse),
        (status = 422, description = "リクエストボディに必須フィールドが無い等", body = ErrorResponse),
        (status = 503, description = "Google ログインを受け付けない構成", body = ErrorResponse),
    )
)]
async fn app_google_login(
    State(state): State<AppState>,
    AppJson(payload): AppJson<AppGoogleLoginRequest>,
) -> Result<Json<AppLoginResponse>, AppError> {
    if !state.google_login.enabled() {
        return Err(AppError::LoginProviderDisabled);
    }
    let account = google_account(&state, &payload.server_auth_code, None)
        .await
        .map_err(GoogleAccountError::app_error)?;
    complete_app_login(&state, &account, &RevocationTokens::default())
        .await
        .map(Json)
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct AppLineLoginRequest {
    /// LINE SDK が返す ID トークン。
    id_token: String,
    /// LINE SDK が返すアクセストークン。退会・連携の解除のときに、LINE との連携の取り消しに使う。
    access_token: String,
    /// `/app/auth/nonce` で受け取り、LINE SDK に渡した値。
    nonce: String,
}

/// LINE でログインする。ID トークンを LINE の検証エンドポイントで確かめ、アクセストークンは
/// 同じ人のものか確かめてから、連携の取り消しのために暗号化して保存する (docs/mobile-app.md)。
#[utoipa::path(
    post,
    path = "/app/auth/line",
    request_body = AppLineLoginRequest,
    responses(
        (status = 200, description = "ログイン成功", body = AppLoginResponse),
        (status = 400, description = "リクエストボディが不正 (JSON として解釈できない等)", body = ErrorResponse),
        (status = 401, description = "LINE で本人を確かめられなかった (`nonce` が発行したものでない・使用済み・期限切れを含む)、またはメールアドレスの提供を断られた", body = ErrorResponse),
        (status = 403, description = "アカウントが凍結されている", body = ErrorResponse),
        (status = 409, description = "同じメールアドレスのアカウントに紐付けられない", body = ErrorResponse),
        (status = 415, description = "Content-Type が application/json でない", body = ErrorResponse),
        (status = 422, description = "リクエストボディに必須フィールドが無い等", body = ErrorResponse),
        (status = 503, description = "LINE ログインを受け付けない構成", body = ErrorResponse),
    )
)]
async fn app_line_login(
    State(state): State<AppState>,
    AppJson(payload): AppJson<AppLineLoginRequest>,
) -> Result<Json<AppLoginResponse>, AppError> {
    let line = &state.line_login;
    if !line.enabled() {
        return Err(AppError::LoginProviderDisabled);
    }
    if !state.app_login_nonces.consume(&payload.nonce) {
        return Err(AppError::ExternalLoginFailed);
    }
    let account = line_account(&state, &payload.id_token, &payload.nonce)
        .await
        .ok_or(AppError::ExternalLoginFailed)?;
    let owner = line
        .access_token_user_id(&payload.access_token)
        .await
        .map_err(|err| {
            tracing::warn!(error = %crate::error_chain_line(&err), "LINE のアクセストークンの持ち主を確かめられませんでした");
            AppError::ExternalLoginFailed
        })?;
    // 別の人のアクセストークンを保存すると、退会のときにその人の連携を取り消してしまう。
    if owner != account.subject {
        tracing::warn!("LINE のアクセストークンが、ID トークンの本人のものではありませんでした");
        return Err(AppError::ExternalLoginFailed);
    }
    // ウェブと同じく、メールアドレスを受け取れなければログインさせない (docs/authentication.md)。
    if account.email.is_none() {
        return Err(AppError::LineEmailRequired);
    }

    let revocation_tokens = RevocationTokens {
        refresh_token: None,
        access_token: line.seal_token(&payload.access_token),
    };
    complete_app_login(&state, &account, &revocation_tokens)
        .await
        .map(Json)
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct AppAppleLoginRequest {
    /// Sign in with Apple が返す認可コード (`authorizationCode`)。
    authorization_code: String,
    /// `/app/auth/nonce` で受け取り、Sign in with Apple の要求に渡した値。
    nonce: String,
    /// 最初の承認のときだけ Apple が渡す名前 (`fullName` の名・姓)。
    #[serde(default)]
    given_name: Option<String>,
    #[serde(default)]
    family_name: Option<String>,
}

/// Sign in with Apple でログインする。認可コードを Bundle ID 宛てに引き換え、ID トークンの
/// `nonce` を確かめる (docs/authentication.md)。
#[utoipa::path(
    post,
    path = "/app/auth/apple",
    request_body = AppAppleLoginRequest,
    responses(
        (status = 200, description = "ログイン成功", body = AppLoginResponse),
        (status = 400, description = "リクエストボディが不正 (JSON として解釈できない等)", body = ErrorResponse),
        (status = 401, description = "Apple で本人を確かめられなかった (`nonce` が発行したものでない・使用済み・期限切れを含む)", body = ErrorResponse),
        (status = 403, description = "アカウントが凍結されている", body = ErrorResponse),
        (status = 409, description = "同じメールアドレスのアカウントに紐付けられない", body = ErrorResponse),
        (status = 415, description = "Content-Type が application/json でない", body = ErrorResponse),
        (status = 422, description = "リクエストボディに必須フィールドが無い等", body = ErrorResponse),
        (status = 503, description = "アプリの Sign in with Apple を受け付けない構成", body = ErrorResponse),
    )
)]
async fn app_apple_login(
    State(state): State<AppState>,
    AppJson(payload): AppJson<AppAppleLoginRequest>,
) -> Result<Json<AppLoginResponse>, AppError> {
    if !state.apple_login.app_enabled() {
        return Err(AppError::LoginProviderDisabled);
    }
    if !state.app_login_nonces.consume(&payload.nonce) {
        return Err(AppError::ExternalLoginFailed);
    }
    let (account, revocation_tokens) = apple_account(
        &state,
        AppleClient::App,
        &payload.authorization_code,
        &payload.nonce,
        payload.given_name.as_deref(),
        payload.family_name.as_deref(),
    )
    .await
    .ok_or(AppError::ExternalLoginFailed)?;
    complete_app_login(&state, &account, &revocation_tokens)
        .await
        .map(Json)
}

/// 提供元での本人確認が済んだ後、ログインしてよい利用者を決めてトークンを渡す。
async fn complete_app_login(
    state: &AppState,
    account: &ExternalAccount,
    revocation_tokens: &RevocationTokens,
) -> Result<AppLoginResponse, AppError> {
    let user = resolve_external_login(state, account, revocation_tokens)
        .await
        .map_err(ExternalLoginRejection::app_error)?;
    let username = crate::auth::load_username(&state.pool, user.id)
        .await?
        .ok_or(AppError::ExternalLoginFailed)?;
    let token = app_token::establish(&state.pool, user.id).await?;
    Ok(AppLoginResponse {
        token,
        user: UserResponse {
            id: user.id,
            username,
            deletion_cancelled: user.deletion_cancelled,
        },
    })
}

/// 送られてきたトークンを無効にする。トークンが無い・無効なときは何もせず成功にする
/// (ウェブの `/auth/logout` と同じく、ログアウトは失敗させない)。
#[utoipa::path(
    post,
    path = "/app/auth/logout",
    responses((status = 204, description = "ログアウト成功")),
)]
async fn app_logout(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<StatusCode, AppError> {
    if let Some(token) = app_token::bearer(&headers) {
        app_token::revoke(&state.pool, token).await?;
    }
    Ok(StatusCode::NO_CONTENT)
}

pub fn router() -> OpenApiRouter<AppState> {
    OpenApiRouter::new()
        .routes(routes!(app_login))
        .routes(routes!(app_login_nonce))
        .routes(routes!(app_google_login))
        .routes(routes!(app_line_login))
        .routes(routes!(app_apple_login))
        .routes(routes!(app_logout))
}
