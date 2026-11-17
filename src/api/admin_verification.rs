//! 管理画面に入るときの確認コード (docs/authentication.md「管理者の確認コード」、`crate::admin_totp`)。

use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use serde::{Deserialize, Serialize};
use tower_sessions::Session;
use utoipa::ToSchema;
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;

use crate::admin_totp::{self, AdminTotp, Credential, Verification};
use crate::auth::{AuthUser, Role};
use crate::error::{AppError, AppJson, ErrorResponse};
use crate::forwarded::ClientIp;
use crate::state::AppState;

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct AdminVerificationStatus {
    /// 管理 API に確認コードを求める構成か。`false` なら、画面は入力を出さずに管理画面を開く。
    required: bool,
    /// 認証アプリが端末で登録済みか。`false` なら、画面は端末での登録を案内する。
    enrolled: bool,
    /// このログインで確認コードを通してから1時間の内か。
    verified: bool,
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct AdminVerificationRequest {
    /// 認証アプリに出た6桁。
    #[schema(example = "123456")]
    code: String,
}

/// 管理者で、確認コードを通す前でも使える。それ以外は 403。
fn require_admin(user: &AuthUser) -> Result<(), AppError> {
    if user.role == Role::Admin {
        Ok(())
    } else {
        Err(AppError::Forbidden)
    }
}

fn credential<'a>(user: &AuthUser, session: &'a Session) -> Credential<'a> {
    match user.app_token_id {
        Some(id) => Credential::AppToken(id),
        None => Credential::Session(session),
    }
}

/// 確認コードの状態。管理画面を開いたときに、入力を出すかを決めるのに使う。
#[utoipa::path(
    get,
    path = "/admin/verification",
    responses(
        (status = 200, description = "確認コードの状態", body = AdminVerificationStatus),
        (status = 401, description = "未ログイン", body = ErrorResponse),
        (status = 403, description = "管理者ではない", body = ErrorResponse),
    )
)]
async fn status(
    user: AuthUser,
    session: Session,
    State(state): State<AppState>,
) -> Result<Json<AdminVerificationStatus>, AppError> {
    require_admin(&user)?;
    if state.admin_totp.is_none() {
        return Ok(Json(AdminVerificationStatus {
            required: false,
            enrolled: false,
            verified: true,
        }));
    }
    Ok(Json(AdminVerificationStatus {
        required: true,
        enrolled: AdminTotp::is_enrolled(&state.pool, user.id).await?,
        verified: admin_totp::is_verified(&state.pool, credential(&user, &session)).await?,
    }))
}

/// 確認コードを通す。通ったら、このログインで1時間、管理 API を使える。
///
/// 照合は管理者ごとに1分5回まで (6桁は総当たりできるため)。誤りは運営者に知らせる。
#[utoipa::path(
    post,
    path = "/admin/verification",
    request_body = AdminVerificationRequest,
    responses(
        (status = 204, description = "通した (確認コードを求めない構成では、何もせずに 204)"),
        (status = 400, description = "確認コードが違う・使用済み、またはリクエストボディが不正", body = ErrorResponse),
        (status = 401, description = "未ログイン", body = ErrorResponse),
        (status = 403, description = "管理者ではない", body = ErrorResponse),
        (status = 409, description = "認証アプリが端末で登録されていない", body = ErrorResponse),
        (status = 415, description = "Content-Type が application/json でない", body = ErrorResponse),
        (status = 422, description = "必須フィールドが無い", body = ErrorResponse),
        (status = 429, description = "照合の回数の上限を超えた", body = ErrorResponse),
    )
)]
async fn verify(
    user: AuthUser,
    session: Session,
    State(state): State<AppState>,
    ClientIp(client_ip): ClientIp,
    AppJson(payload): AppJson<AdminVerificationRequest>,
) -> Result<StatusCode, AppError> {
    require_admin(&user)?;
    let Some(totp) = &state.admin_totp else {
        return Ok(StatusCode::NO_CONTENT);
    };
    let key = user.id.to_string();
    let reservation = state
        .admin_code_rate_limiter
        .try_acquire(&key)
        .ok_or(AppError::TooManyRequests)?;
    let verification = totp
        .verify(&state.pool, user.id, &user.username, &payload.code)
        .await
        .map_err(|err| match err {
            admin_totp::Error::Db(err) => AppError::Database(err),
            // 鍵が変わったなどで読めない登録は、無いものとして登録し直してもらう。
            admin_totp::Error::Unreadable => {
                tracing::error!(error = %crate::error_chain_line(&err), user_id = user.id, "確認コードの登録を読めません");
                AppError::AdminCodeNotEnrolled
            }
        })?;
    match verification {
        Verification::Passed => {
            state.admin_code_rate_limiter.release(&key, reservation);
            admin_totp::mark_verified(&state.pool, credential(&user, &session)).await?;
            Ok(StatusCode::NO_CONTENT)
        }
        Verification::WrongCode => {
            state
                .login_failures
                .record_admin_code_failure(user.id, client_ip);
            Err(AppError::InvalidAdminCode)
        }
        Verification::NotEnrolled => Err(AppError::AdminCodeNotEnrolled),
    }
}

pub fn router() -> OpenApiRouter<AppState> {
    OpenApiRouter::new().routes(routes!(status, verify))
}
