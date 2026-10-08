//! メールで届くリンクを使う API (サインアップ・パスワードの再設定・メールアドレスの変更) の共通部分。

use std::net::IpAddr;

use crate::auth;
use crate::email_token::{self, Purpose};
use crate::error::AppError;
use crate::forwarded;
use crate::state::AppState;
use crate::validation;

/// メールのリンクの行き先。アプリからの申し込みなら `app=1` を付け、リンクの先 (ブラウザで開く)
/// で「アプリに戻ってログイン」と案内させる (docs/mobile-app.md)。
pub fn link_path(page: &str, token: &str, from_app: bool) -> String {
    let app = if from_app { "&app=1" } else { "" };
    format!("{page}?token={token}{app}")
}

/// メールを送る申し込みを受け付け、送り先のユーザーIDを返す。
pub fn accept_mail_request(
    state: &AppState,
    client_ip: Option<IpAddr>,
    email: &str,
) -> Result<String, AppError> {
    let username = username_from_email(email)?;
    state
        .mail_request_rate_limiter
        .try_acquire(&forwarded::rate_limit_key_or_shared(client_ip))
        .ok_or(AppError::TooManyRequests)?;
    Ok(username)
}

/// 申し込みのメールアドレスを、保存されている形のユーザーIDにする。
///
/// メールアドレスの書式を満たしても、ユーザーIDとして使えないもの (空白を含む・長すぎる) は
/// 管理者による作成と同じ条件で弾く。
fn username_from_email(email: &str) -> Result<String, AppError> {
    let address = email
        .trim()
        .parse::<lettre::Address>()
        .map_err(|_| AppError::Validation("メールアドレスとして解釈できません".to_string()))?;
    let username = auth::normalize_username(address.as_ref());
    validation::validate_username(&username).map_err(super::username_error)?;
    Ok(username)
}

/// リンクの先で設定する新しいパスワード (確認完了・再設定) を検証し、トークンを使い済みにして、
/// 紐付くユーザーの id とパスワードのハッシュを返す。
///
/// パスワードの検証はトークンを使い済みにする前に行う。ポリシー違反の入力でリンクを潰さないため。
///
/// 枠 (`AppState::link_password_rate_limiter`) はトークンを使い済みにする前に確保し、
/// Argon2 まで進まない結果 (使えないトークン・DB エラー) では返す。
/// - 後で確保すると、上限で弾かれた要求がリンクを潰す
/// - 返さないと、でたらめなトークンを送るだけで全体の枠を使い切り、他の人の操作を止められる
///
/// トークンを確かめるだけにせず使い済みにするのは、同じトークンで並行に送られた要求がすべて
/// Argon2 を回せる (CPU 枯渇の経路になる) ため。この後で失敗するとリンクは使えなくなるが、
/// 申し込み直せば発行し直せる。
pub async fn consume_for_new_password(
    state: &AppState,
    client_ip: Option<IpAddr>,
    token: &str,
    purpose: Purpose,
    password: String,
) -> Result<(i64, String), AppError> {
    auth::validate_password(&state.password, &password)?;

    let client_key = forwarded::rate_limit_key_or_shared(client_ip);
    let reservation = state
        .link_password_rate_limiter
        .try_acquire(&client_key)
        .ok_or(AppError::TooManyRequests)?;

    let user_id = match email_token::consume(&state.pool, token, purpose).await {
        Ok(Some(user_id)) => user_id,
        outcome => {
            state
                .link_password_rate_limiter
                .release(&client_key, reservation);
            outcome?;
            return Err(AppError::InvalidEmailToken);
        }
    };
    let password_hash = auth::hash_password_async(password).await?;
    Ok((user_id, password_hash))
}

/// リンクの先を開いたときに、リンクがまだ使えるかを尋ねる要求。
#[derive(Debug, serde::Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CheckLinkRequest {
    /// メールのリンクに載っていたトークン。
    token: String,
}

impl CheckLinkRequest {
    pub fn token(&self) -> &str {
        &self.token
    }
}

/// リンクがまだ使えるかを、使用済みにせずに確かめる。使えなければ `InvalidEmailToken`。
///
/// ADR: パスワードを入れて送った後ではなく、リンクを開いた時点で期限切れ・使用済みを知らせ、
/// 申し込み直す先へ案内するため (docs/authentication.md)。トークンは推測できない長さなので、
/// 確かめる手段を増やしても当てられるようにはならない。
pub async fn check_link(
    state: &AppState,
    request: &CheckLinkRequest,
    purpose: Purpose,
) -> Result<(), AppError> {
    if email_token::is_usable(&state.pool, &request.token, purpose).await? {
        Ok(())
    } else {
        Err(AppError::InvalidEmailToken)
    }
}
