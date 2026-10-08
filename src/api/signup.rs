//! オープンサインアップとメール確認 (docs/authentication.md)。

use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use serde::Deserialize;
use tower_sessions::Session;
use utoipa::ToSchema;
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;

use super::email_link;
use crate::app_token;
use crate::auth;
use crate::email_token::Purpose;
use crate::error::{AppError, AppJson, ErrorResponse};
use crate::forwarded::ClientIp;
use crate::mail::{self, LINK_HELP};
use crate::signup::{self, Registration};
use crate::state::AppState;
use crate::terms;

#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SignupRequest {
    #[schema(example = "kyoko@example.com")]
    email: String,
}

/// サインアップを受け付け、メールを送る。
///
/// 応答は登録の有無に関わらず同じにし、違いはメールの中身だけにする (メールアドレスから利用の
/// 有無を探らせないため)。未登録・確認待ちなら確認リンク、確認済みならログインの案内を送る。
/// 確認リンクを直前に送っていれば、発行し直さず何も送らない。
#[utoipa::path(
    post,
    path = "/auth/signup",
    request_body = SignupRequest,
    responses(
        (status = 204, description = "受け付けた (登録済みのメールアドレスでも同じ応答)"),
        (status = 400, description = "リクエストボディが不正 (JSON として解釈できない等)", body = ErrorResponse),
        (status = 415, description = "Content-Type が application/json でない", body = ErrorResponse),
        (status = 422, description = "メールアドレスとして解釈できない、または必須フィールドが無い", body = ErrorResponse),
        (status = 429, description = "同じ接続元からの要求が上限を超えた", body = ErrorResponse),
    )
)]
async fn signup(
    State(state): State<AppState>,
    ClientIp(client_ip): ClientIp,
    headers: HeaderMap,
    AppJson(payload): AppJson<SignupRequest>,
) -> Result<StatusCode, AppError> {
    let username = email_link::accept_mail_request(&state, client_ip, &payload.email)?;
    let from_app = app_token::is_from_app(&headers);

    // ADR: 受け付けから送信までを、応答を待たずに行う (再設定と同じ)。確認リンクを直前に送った
    // 確認待ちのアドレスだけメールを送らないため、待つと続けて2回申し込んだときの応答時間
    // (や、送信の失敗の応答) から登録の有無が分かる。送れなかったことは利用者に伝えられないが、
    // 届かなければ申し込み直せる。
    tokio::spawn(send_link(state, username, from_app));
    Ok(StatusCode::NO_CONTENT)
}

/// サインアップを受け付け、登録の状態に応じたメールを送る。
async fn send_link(state: AppState, username: String, from_app: bool) {
    let unusable_hash = match auth::unusable_password_hash().await {
        Ok(hash) => hash,
        Err(err) => {
            tracing::error!(error = %crate::error_chain_line(&err), "サインアップのパスワードハッシュを作れませんでした");
            return;
        }
    };
    let registration = match signup::register(
        &state.pool,
        &username,
        &unusable_hash,
        terms::VERSION,
    )
    .await
    {
        Ok(registration) => registration,
        Err(err) => {
            tracing::error!(error = %crate::error_chain_line(&err), "サインアップを受け付けられませんでした");
            return;
        }
    };
    let sent = async {
        let (subject, body) = match registration {
            Registration::Unverified { token } => {
                let link = state
                    .mailer
                    .link(&email_link::link_path(VERIFY_PATH, &token, from_app));
                (
                    VERIFY_SUBJECT,
                    verify_body("BP Carnet のアカウント作成を受け付けました。", &link),
                )
            }
            // 同じリンクは送り直せない (トークンはハッシュしか持たない)。
            Registration::LinkRecentlyIssued => return Ok::<_, mail::Error>(()),
            Registration::AlreadyRegistered => {
                // アプリからの申し込みなら、ブラウザのログイン画面ではなくアプリへ戻す。
                let login = if from_app {
                    APP_LOGIN_GUIDE.to_string()
                } else {
                    format!(
                        "次のリンクからログインしてください。\n\n{}\n",
                        state.mailer.link("/login")
                    )
                };
                (ALREADY_REGISTERED_SUBJECT, already_registered_body(&login))
            }
        };
        state.mailer.send(&username, subject, body).await
    };
    if let Err(err) = sent.await {
        tracing::error!(error = %crate::error_chain_line(&err), "サインアップのメールを送れませんでした");
    }
}

pub(super) const VERIFY_PATH: &str = "/verify-email";

pub(super) const VERIFY_SUBJECT: &str = "【BP Carnet】アカウント作成の確認";

/// `lead` は1行目 (何を受け付けたか)。確認前のアカウントでの再設定の申し込みでも送る。
pub(super) fn verify_body(lead: &str, link: &str) -> String {
    format!(
        "{lead}\n\
         次のリンクを開いて、パスワードを設定してください。リンクの有効期限は24時間です。\n\
         \n\
         {link}\n\
         \n\
         {LINK_HELP}\
         有効期限が過ぎた場合は、もう一度アカウントの作成をお申し込みください。\n\
         このメールに心当たりが無い場合は、何もせずに破棄してください。アカウントは作成されません。\n"
    )
}

const ALREADY_REGISTERED_SUBJECT: &str = "【BP Carnet】アカウント作成のお申し込みについて";

const APP_LOGIN_GUIDE: &str = "BP Carnet のアプリに戻って、ログインしてください。\n";

/// `login` はログインの案内 (リンクか、アプリに戻る案内)。
fn already_registered_body(login: &str) -> String {
    format!(
        "BP Carnet のアカウント作成のお申し込みがありましたが、このメールアドレスは既に登録されています。\n\
         {login}\
         \n\
         パスワードを忘れた場合は、ログイン画面の「パスワードの再設定」から設定し直せます。\n\
         このメールに心当たりが無い場合は、何もせずに破棄してください。\n"
    )
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CompleteSignupRequest {
    /// 確認メールのリンクに載っていたトークン。
    token: String,
    password: String,
}

/// 確認リンクの先でパスワードを設定し、メールアドレスを確認済みにしてログインさせる。
#[utoipa::path(
    post,
    path = "/auth/signup/complete",
    request_body = CompleteSignupRequest,
    responses(
        (status = 204, description = "パスワードを設定し、ログインした"),
        (status = 400, description = "リンクが無効・期限切れ・使用済み、またはリクエストボディが不正", body = ErrorResponse),
        (status = 415, description = "Content-Type が application/json でない", body = ErrorResponse),
        (status = 422, description = "パスワードがポリシーを満たさない、または必須フィールドが無い", body = ErrorResponse),
        (status = 429, description = "同じ接続元、または全体からの要求が上限を超えた", body = ErrorResponse),
    )
)]
async fn complete_signup(
    session: Session,
    State(state): State<AppState>,
    ClientIp(client_ip): ClientIp,
    AppJson(payload): AppJson<CompleteSignupRequest>,
) -> Result<StatusCode, AppError> {
    let (user_id, password_hash) = email_link::consume_for_new_password(
        &state,
        client_ip,
        &payload.token,
        Purpose::VerifyEmail,
        payload.password,
    )
    .await?;
    if !signup::complete(&state.pool, user_id, &password_hash).await? {
        return Err(AppError::InvalidEmailToken);
    }
    tracing::info!(user_id, "サインアップのメール確認が済みました");

    auth::establish_session(&session, &state.pool, user_id).await?;
    Ok(StatusCode::NO_CONTENT)
}

/// 確認リンクがまだ使えるかを、使用済みにせずに確かめる (リンクを開いたときに使う)。
#[utoipa::path(
    post,
    path = "/auth/signup/check",
    request_body = email_link::CheckLinkRequest,
    responses(
        (status = 204, description = "リンクは使える"),
        (status = 400, description = "リンクが無効・期限切れ・使用済み、またはリクエストボディが不正", body = ErrorResponse),
        (status = 415, description = "Content-Type が application/json でない", body = ErrorResponse),
        (status = 422, description = "必須フィールドが無い", body = ErrorResponse),
    )
)]
async fn check_signup_link(
    State(state): State<AppState>,
    AppJson(payload): AppJson<email_link::CheckLinkRequest>,
) -> Result<StatusCode, AppError> {
    email_link::check_link(&state, &payload, Purpose::VerifyEmail).await?;
    Ok(StatusCode::NO_CONTENT)
}

pub fn router() -> OpenApiRouter<AppState> {
    OpenApiRouter::new()
        .routes(routes!(signup))
        .routes(routes!(complete_signup))
        .routes(routes!(check_signup_link))
}
