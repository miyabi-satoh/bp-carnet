//! メールアドレスの変更 (docs/authentication.md)。

use axum::Json;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;

use super::email_link;
use crate::app_token;
use crate::auth::{self, AuthUser};
use crate::email_change;
use crate::email_token;
use crate::error::{AppError, AppJson, ErrorResponse};
use crate::forwarded::ClientIp;
use crate::mail::LINK_HELP;
use crate::state::AppState;

#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct RequestEmailChangeRequest {
    #[schema(example = "new@example.com")]
    new_email: String,
    /// 今のパスワード。パスワードを持つアカウントだけ必須 (持たないアカウントは送らない)。
    current_password: Option<String>,
}

/// 新しいメールアドレスに、変更を確かめるリンクを送る。
///
/// パスワードを持つアカウントは、今のパスワードを求める (パスワードの変更と同じく、セッションだけを
/// 奪った相手に変えられないようにするため)。応答は新しいアドレスが使われているかによらず同じにし、
/// 違いはメールの中身だけにする (サインアップと同じく、アドレスから利用の有無を探らせないため)。
#[utoipa::path(
    post,
    path = "/account/email",
    request_body = RequestEmailChangeRequest,
    responses(
        (status = 204, description = "受け付けた (新しいアドレスが使われていても同じ応答)"),
        (status = 400, description = "リクエストボディが不正 (JSON として解釈できない等)", body = ErrorResponse),
        (status = 401, description = "未ログイン", body = ErrorResponse),
        (status = 415, description = "Content-Type が application/json でない", body = ErrorResponse),
        (status = 422, description = "メールアドレスとして解釈できない・今のアドレスと同じ・今のパスワードが一致しない", body = ErrorResponse),
        (status = 429, description = "要求か、今のパスワードの照合の試行回数が上限を超えた", body = ErrorResponse),
    )
)]
async fn request_email_change(
    user: AuthUser,
    State(state): State<AppState>,
    ClientIp(client_ip): ClientIp,
    headers: HeaderMap,
    AppJson(payload): AppJson<RequestEmailChangeRequest>,
) -> Result<StatusCode, AppError> {
    let new_email = email_link::accept_mail_request(&state, client_ip, &payload.new_email)?;
    if new_email == user.username {
        return Err(AppError::Validation(
            "今のメールアドレスと同じです".to_string(),
        ));
    }
    check_current_password(&state, &user, payload.current_password).await?;

    let from_app = app_token::is_from_app(&headers);
    // サインアップと同じく、送るまでを応答を待たずに行う (応答時間から利用の有無が分からないように)。
    tokio::spawn(send_link(state, user.id, new_email, from_app));
    Ok(StatusCode::NO_CONTENT)
}

/// パスワードを持つアカウントなら、今のパスワードを照合する。
async fn check_current_password(
    state: &AppState,
    user: &AuthUser,
    current_password: Option<String>,
) -> Result<(), AppError> {
    let row = sqlx::query!(
        r#"SELECT password_hash, password_usable AS "password_usable!: bool" FROM users WHERE id = ?"#,
        user.id
    )
    .fetch_one(&state.pool)
    .await?;
    if !row.password_usable {
        return Ok(());
    }
    let password = current_password.ok_or(AppError::IncorrectPassword)?;
    // パスワードの変更と同じ枠を使う (総当たりと Argon2 の連続実行を防ぐため)。
    let reservation = state
        .password_change_rate_limiter
        .try_acquire(&user.username)
        .ok_or(AppError::TooManyRequests)?;
    if !auth::verify_password_async(password, row.password_hash).await? {
        return Err(AppError::IncorrectPassword);
    }
    state
        .password_change_rate_limiter
        .release(&user.username, reservation);
    Ok(())
}

/// 新しいアドレスが確認済みのアカウントで使われていれば、変えられないことを知らせる。
/// そうでなければ確かめるリンクを送る。
async fn send_link(state: AppState, user_id: i64, new_email: String, from_app: bool) {
    let mail = async {
        if auth::find_verified_user_id(&state.pool, &new_email)
            .await?
            .is_some()
        {
            return Ok::<_, sqlx::Error>(Some((TAKEN_SUBJECT, taken_body())));
        }
        let Some(token) = email_token::issue_change_email(&state.pool, user_id, &new_email).await?
        else {
            // 同じアドレスに直前に送ったリンクが、まだ使われていない。
            return Ok(None);
        };
        let link = state
            .mailer
            .link(&email_link::link_path(LINK_PATH, &token, from_app));
        Ok(Some((LINK_SUBJECT, link_body(&link))))
    };
    let (subject, body) = match mail.await {
        Ok(Some(mail)) => mail,
        Ok(None) => return,
        Err(err) => {
            tracing::error!(error = %crate::error_chain_line(&err), "メールアドレスの変更のリンクを発行できませんでした");
            return;
        }
    };
    if let Err(err) = state.mailer.send(&new_email, subject, body).await {
        tracing::error!(error = %crate::error_chain_line(&err), "メールアドレスの変更のメールを送れませんでした");
    }
}

const LINK_PATH: &str = "/change-email";

const LINK_SUBJECT: &str = "【BP Carnet】メールアドレスの変更の確認";

fn link_body(link: &str) -> String {
    format!(
        "BP Carnet のメールアドレスを、このアドレスに変更するお申し込みを受け付けました。\n\
         次のリンクを開いて、変更を確定してください。リンクの有効期限は1時間です。\n\
         \n\
         {link}\n\
         \n\
         {LINK_HELP}\
         このメールに心当たりが無い場合は、何もせずに破棄してください。メールアドレスは変更されません。\n"
    )
}

const TAKEN_SUBJECT: &str = "【BP Carnet】メールアドレスの変更のお申し込みについて";

fn taken_body() -> String {
    "BP Carnet のメールアドレスを、このアドレスに変更するお申し込みがありましたが、\
     このアドレスは既に別のアカウントで使われているため、変更できません。\n\
     \n\
     このメールに心当たりが無い場合は、何もせずに破棄してください。\n"
        .to_string()
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct EmailChangeTargetResponse {
    /// 変更先のメールアドレス。
    new_email: String,
}

/// 変更のリンクがまだ使えれば、変更先のアドレスを返す (リンクを開いたときに見せて確かめてもらう)。
#[utoipa::path(
    post,
    path = "/auth/email-change/check",
    request_body = email_link::CheckLinkRequest,
    responses(
        (status = 200, description = "リンクは使える", body = EmailChangeTargetResponse),
        (status = 400, description = "リンクが無効・期限切れ・使用済み、またはリクエストボディが不正", body = ErrorResponse),
        (status = 415, description = "Content-Type が application/json でない", body = ErrorResponse),
        (status = 422, description = "必須フィールドが無い", body = ErrorResponse),
    )
)]
async fn check_email_change_link(
    State(state): State<AppState>,
    AppJson(payload): AppJson<email_link::CheckLinkRequest>,
) -> Result<Json<EmailChangeTargetResponse>, AppError> {
    let new_email = email_token::change_email_target(&state.pool, payload.token())
        .await?
        .ok_or(AppError::InvalidEmailToken)?;
    Ok(Json(EmailChangeTargetResponse { new_email }))
}

/// リンクの先で変更を確定し、古いアドレスに知らせる。ログインしていなくても確定できる
/// (リンクが新しいアドレスを受け取れることの確かめになるため。サインアップの確認と同じ)。
#[utoipa::path(
    post,
    path = "/auth/email-change/complete",
    request_body = email_link::CheckLinkRequest,
    responses(
        (status = 204, description = "変更した"),
        (status = 400, description = "リンクが無効・期限切れ・使用済み、またはリクエストボディが不正", body = ErrorResponse),
        (status = 409, description = "新しいアドレスが、申し込みの後に別のアカウントで使われた", body = ErrorResponse),
        (status = 415, description = "Content-Type が application/json でない", body = ErrorResponse),
        (status = 422, description = "必須フィールドが無い", body = ErrorResponse),
    )
)]
async fn complete_email_change(
    State(state): State<AppState>,
    AppJson(payload): AppJson<email_link::CheckLinkRequest>,
) -> Result<StatusCode, AppError> {
    let (user_id, new_email) = email_token::consume_change_email(&state.pool, payload.token())
        .await?
        .ok_or(AppError::InvalidEmailToken)?;
    let old = email_change::complete(&state.pool, user_id, &new_email)
        .await?
        .ok_or(AppError::InvalidEmailToken)?;
    tracing::info!(user_id, "メールアドレスを変更しました");

    // 古いアドレスがメールアドレスなら知らせる (乗っ取りで変えられたときに、本人が気づけるように)。
    if state.mailer.can_send_to(&old) {
        let mailer = state.mailer.clone();
        let body = notice_body(&new_email, &state.contact_url);
        tokio::spawn(async move {
            if let Err(err) = mailer.send(&old, NOTICE_SUBJECT, body).await {
                tracing::error!(error = %crate::error_chain_line(&err), user_id, "メールアドレスの変更を古いアドレスに知らせられませんでした");
            }
        });
    }
    Ok(StatusCode::NO_CONTENT)
}

const NOTICE_SUBJECT: &str = "【BP Carnet】メールアドレスが変更されました";

fn notice_body(new_email: &str, contact_url: &str) -> String {
    format!(
        "BP Carnet のメールアドレスが、{new_email} に変更されました。\n\
         これからは、新しいメールアドレスでログインしてください。このアドレスには、今後のお知らせは届きません。\n\
         \n\
         この変更に心当たりが無い場合は、アカウントが第三者に使われているおそれがあります。\n\
         次のお問い合わせ先までご連絡ください。\n\
         {contact_url}\n"
    )
}

pub fn router() -> OpenApiRouter<AppState> {
    OpenApiRouter::new()
        .routes(routes!(request_email_change))
        .routes(routes!(check_email_change_link))
        .routes(routes!(complete_email_change))
}
