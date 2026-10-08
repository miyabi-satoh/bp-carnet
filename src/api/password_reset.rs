//! メールでのパスワードの再設定 (docs/authentication.md)。

use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use serde::Deserialize;
use utoipa::ToSchema;
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;

use super::email_link;
use super::signup::{VERIFY_PATH, VERIFY_SUBJECT, verify_body};
use crate::app_token;
use crate::email_token::Purpose;
use crate::error::{AppError, AppJson, ErrorResponse};
use crate::forwarded::ClientIp;
use crate::mail::LINK_HELP;
use crate::password_notice;
use crate::password_reset;
use crate::signup;
use crate::state::AppState;

#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct PasswordResetRequest {
    #[schema(example = "kyoko@example.com")]
    email: String,
}

/// 再設定を受け付け、メール確認済みのアカウントがあれば再設定のリンクを、確認前のアカウント
/// なら確認リンクを送る。
///
/// 応答はアカウントの有無に関わらず同じにする。アカウントが無いアドレスにはメールを送らない
/// (無関係なアドレスへの送りつけに使わせないため)。
#[utoipa::path(
    post,
    path = "/auth/password-reset",
    request_body = PasswordResetRequest,
    responses(
        (status = 204, description = "受け付けた (アカウントが無いメールアドレスでも同じ応答)"),
        (status = 400, description = "リクエストボディが不正 (JSON として解釈できない等)", body = ErrorResponse),
        (status = 415, description = "Content-Type が application/json でない", body = ErrorResponse),
        (status = 422, description = "メールアドレスとして解釈できない、または必須フィールドが無い", body = ErrorResponse),
        (status = 429, description = "同じ接続元からの要求が上限を超えた", body = ErrorResponse),
    )
)]
async fn request_password_reset(
    State(state): State<AppState>,
    ClientIp(client_ip): ClientIp,
    headers: HeaderMap,
    AppJson(payload): AppJson<PasswordResetRequest>,
) -> Result<StatusCode, AppError> {
    let username = email_link::accept_mail_request(&state, client_ip, &payload.email)?;
    let from_app = app_token::is_from_app(&headers);

    // ADR: アカウントの検索から送信までを、応答を待たずに行う。アカウントがあるときだけトークンの
    // 書き込みと SMTP の往復が挟まるため、待つと応答時間 (や、その途中の失敗の応答) から登録の
    // 有無が分かる。送れなかったことは利用者に伝えられないが、届かなければ申し込み直せる。
    tokio::spawn(send_link(state, username, from_app));
    Ok(StatusCode::NO_CONTENT)
}

/// 再設定のリンクを発行して送る。確認前のアカウントなら、代わりに確認リンクを送り直す。
/// 発行しない場合 (アカウントが無い等) は何もしない。
///
/// ADR: 確認前のアカウントはパスワードも記録も持たず、サインアップの申し込み直しと同じ結果に
/// なるため、確認メールを失くした人がここで迷わないよう確認リンクを送る。
async fn send_link(state: AppState, username: String, from_app: bool) {
    let issued = async {
        if let Some(token) = password_reset::issue(&state.pool, &username).await? {
            return Ok(Some((Purpose::ResetPassword, token)));
        }
        let token = signup::reissue_verification(&state.pool, &username).await?;
        Ok::<_, sqlx::Error>(token.map(|token| (Purpose::VerifyEmail, token)))
    };
    let (purpose, token) = match issued.await {
        Ok(Some(issued)) => issued,
        Ok(None) => return,
        Err(err) => {
            tracing::error!(error = %crate::error_chain_line(&err), "パスワード再設定のトークンを発行できませんでした");
            return;
        }
    };
    let (path, subject, body): (_, _, fn(&str) -> String) = match purpose {
        Purpose::VerifyEmail => (VERIFY_PATH, VERIFY_SUBJECT, |link| {
            verify_body(
                "BP Carnet のパスワードの再設定を受け付けましたが、このメールアドレスのアカウント作成は、まだ確認が済んでいません。",
                link,
            )
        }),
        // 上で発行するのは再設定と確認の2つだけ。
        Purpose::ResetPassword | Purpose::ChangeEmail => ("/reset-password", SUBJECT, body),
    };
    let sent = async {
        let link = state
            .mailer
            .link(&email_link::link_path(path, &token, from_app));
        state.mailer.send(&username, subject, body(&link)).await
    };
    if let Err(err) = sent.await {
        tracing::error!(error = %crate::error_chain_line(&err), "パスワード再設定のメールを送れませんでした");
    }
}

const SUBJECT: &str = "【BP Carnet】パスワードの再設定";

fn body(link: &str) -> String {
    format!(
        "BP Carnet のパスワードの再設定を受け付けました。\n\
         次のリンクを開いて、新しいパスワードを設定してください。リンクの有効期限は1時間です。\n\
         \n\
         {link}\n\
         \n\
         {LINK_HELP}\
         有効期限が過ぎた場合は、もう一度パスワードの再設定をお申し込みください。\n\
         このメールに心当たりが無い場合は、何もせずに破棄してください。パスワードは変わりません。\n"
    )
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CompletePasswordResetRequest {
    /// 再設定のメールのリンクに載っていたトークン。
    token: String,
    password: String,
}

/// リンクの先で新しいパスワードを設定する。そのアカウントのセッションはすべて断つ。
///
/// ADR: ログインはさせず、ログイン画面から入り直してもらう。凍結中の拒否や削除予約の取り消しと
/// いったログイン時の判定を、ログインの経路だけに保つため。
#[utoipa::path(
    post,
    path = "/auth/password-reset/complete",
    request_body = CompletePasswordResetRequest,
    responses(
        (status = 204, description = "再設定した (ログインはしない)"),
        (status = 400, description = "リンクが無効・期限切れ・使用済み、またはリクエストボディが不正", body = ErrorResponse),
        (status = 415, description = "Content-Type が application/json でない", body = ErrorResponse),
        (status = 422, description = "パスワードがポリシーを満たさない、または必須フィールドが無い", body = ErrorResponse),
        (status = 429, description = "同じ接続元、または全体からの要求が上限を超えた", body = ErrorResponse),
    )
)]
async fn complete_password_reset(
    State(state): State<AppState>,
    ClientIp(client_ip): ClientIp,
    AppJson(payload): AppJson<CompletePasswordResetRequest>,
) -> Result<StatusCode, AppError> {
    let (user_id, password_hash) = email_link::consume_for_new_password(
        &state,
        client_ip,
        &payload.token,
        Purpose::ResetPassword,
        payload.password,
    )
    .await?;
    if !password_reset::complete(&state.pool, user_id, &password_hash).await? {
        return Err(AppError::InvalidEmailToken);
    }
    tracing::info!(user_id, "メールでパスワードを再設定しました");
    password_notice::spawn(&state, user_id, password_notice::Origin::EmailLink);
    Ok(StatusCode::NO_CONTENT)
}

/// 再設定のリンクがまだ使えるかを、使用済みにせずに確かめる (リンクを開いたときに使う)。
#[utoipa::path(
    post,
    path = "/auth/password-reset/check",
    request_body = email_link::CheckLinkRequest,
    responses(
        (status = 204, description = "リンクは使える"),
        (status = 400, description = "リンクが無効・期限切れ・使用済み、またはリクエストボディが不正", body = ErrorResponse),
        (status = 415, description = "Content-Type が application/json でない", body = ErrorResponse),
        (status = 422, description = "必須フィールドが無い", body = ErrorResponse),
    )
)]
async fn check_password_reset_link(
    State(state): State<AppState>,
    AppJson(payload): AppJson<email_link::CheckLinkRequest>,
) -> Result<StatusCode, AppError> {
    email_link::check_link(&state, &payload, Purpose::ResetPassword).await?;
    Ok(StatusCode::NO_CONTENT)
}

pub fn router() -> OpenApiRouter<AppState> {
    OpenApiRouter::new()
        .routes(routes!(request_password_reset))
        .routes(routes!(complete_password_reset))
        .routes(routes!(check_password_reset_link))
}
