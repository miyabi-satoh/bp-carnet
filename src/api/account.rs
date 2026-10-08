//! 本人によるアカウント操作 (`/api/v1/account/*`)。

use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use serde::{Deserialize, Serialize};
use tower_sessions::Session;
use utoipa::ToSchema;
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;

use crate::account::{self, DeletionOrigin};
use crate::auth::{self, AuthUser};
use crate::error::{AppError, AppJson, AppPath, ErrorResponse};
use crate::oauth_identity::{self, Provider, Unlinked};
use crate::password_notice;
use crate::state::AppState;

/// 削除の予約結果。管理者による削除 (`api::admin`) も同じフローを通るため同じ形で返す。
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DeletionResponse {
    /// 削除予定日時 (UTC、RFC3339)。この日時を過ぎると記録ごと物理削除される。
    #[schema(example = "2026-10-12T00:00:00.000Z")]
    pub deletion_scheduled_at: String,
    /// 猶予期間 (日数)。確認画面の文言と同じ日数を返す。
    #[schema(example = 30)]
    pub grace_days: i64,
}

/// 自分のアカウントの削除を予約する。
///
/// 即時には削除せず、猶予期間を置く (docs/authentication.md)。猶予期間中にログインすると
/// 予約は自動的に取り消される。誤操作を防ぐ確認文字列の入力は画面側で行う。
///
/// 予約済みのユーザーが再度呼んでも予定日は変わらない (最初の予定をそのまま返す)。
#[utoipa::path(
    post,
    path = "/account/deletion",
    responses(
        (status = 200, description = "削除を予約した", body = DeletionResponse),
        (status = 401, description = "未ログイン", body = ErrorResponse),
        (status = 404, description = "ログイン中のユーザーが既に存在しない", body = ErrorResponse),
    )
)]
async fn schedule_deletion(
    user: AuthUser,
    State(state): State<AppState>,
) -> Result<Json<DeletionResponse>, AppError> {
    let mut tx = state.pool.begin().await?;
    let scheduled = account::schedule_deletion(&mut tx, user.id, DeletionOrigin::User)
        .await?
        .ok_or(AppError::NotFound)?;
    tx.commit().await?;
    tracing::info!(user_id = user.id, "アカウントの削除を予約しました");

    // 送る条件は docs/authentication.md。
    if scheduled.newly_scheduled && state.mailer.can_send_to(&user.username) {
        tokio::spawn(send_deletion_notice(
            state,
            user.id,
            user.username,
            scheduled.scheduled_at.clone(),
        ));
    }

    Ok(Json(DeletionResponse {
        deletion_scheduled_at: scheduled.scheduled_at,
        grace_days: account::DELETION_GRACE_DAYS,
    }))
}

/// 削除の予定日と取り消し方法を本人に知らせる。
///
/// ADR: 応答を待たずに送る。SMTP の往復で削除の操作を待たせないため。送れなくても予約は
/// 成立しているので、ログに残すだけにする。
async fn send_deletion_notice(state: AppState, user_id: i64, to: String, scheduled_at: String) {
    let scheduled: jiff::Timestamp = match scheduled_at.parse() {
        Ok(scheduled) => scheduled,
        Err(_) => {
            tracing::error!(user_id, scheduled_at, "削除予定日時を解釈できませんでした");
            return;
        }
    };
    let date = match account::local_date(&state.pool, user_id, scheduled).await {
        Ok(date) => date,
        Err(err) => {
            tracing::error!(error = %crate::error_chain_line(&err), user_id, "削除予約のメールの予定日を求められませんでした");
            return;
        }
    };
    let sent = async {
        let login_link = state.mailer.link("/login");
        state
            .mailer
            .send(
                &to,
                DELETION_NOTICE_SUBJECT,
                deletion_notice_body(&date, &login_link),
            )
            .await
    };
    if let Err(err) = sent.await {
        tracing::error!(error = %crate::error_chain_line(&err), user_id, "削除予約のメールを送れませんでした");
    }
}

const DELETION_NOTICE_SUBJECT: &str = "【BP Carnet】アカウントの削除を受け付けました";

fn deletion_notice_body(date: &str, login_link: &str) -> String {
    format!(
        "BP Carnet のアカウントの削除を受け付けました。\n\
         {date}以降に、記録データを含めてアカウントを完全に削除します。\n\
         \n\
         それまでにログインすると、削除は自動的に取り消されます。\n\
         \n\
         {login_link}\n\
         \n\
         このメールに心当たりが無い場合は、上のリンクからログインして削除を取り消してください。\n"
    )
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ChangePasswordRequest {
    #[schema(example = "password")]
    current_password: String,
    #[schema(example = "new-password")]
    new_password: String,
}

/// 自分のパスワードを変更する (docs/authentication.md)。
///
/// Google 専用アカウントには絶対に一致しないハッシュが入っているため、現在のパスワードの
/// 照合で必ず弾かれる。この経路を特別扱いする必要は無い。
///
/// 変更すると、この端末以外のセッションは無効になる (docs/authentication.md)。乗っ取りを疑って
/// 変更したときに、奪われたセッションがそのまま残らないようにするため。操作した本人は
/// 続けて使えるよう、この端末のセッションだけ新しい世代に更新する。
#[utoipa::path(
    put,
    path = "/account/password",
    request_body = ChangePasswordRequest,
    responses(
        (status = 204, description = "変更した"),
        (status = 400, description = "リクエストボディが不正 (JSON として解釈できない等)", body = ErrorResponse),
        (status = 401, description = "未ログイン", body = ErrorResponse),
        (status = 415, description = "Content-Type が application/json でない", body = ErrorResponse),
        (status = 422, description = "現在のパスワードが一致しない、または新しいパスワードが設定のポリシーを満たさない", body = ErrorResponse),
        (status = 429, description = "現在のパスワードの照合試行回数の上限を超えた", body = ErrorResponse),
    )
)]
async fn change_password(
    user: AuthUser,
    session: Session,
    State(state): State<AppState>,
    AppJson(payload): AppJson<ChangePasswordRequest>,
) -> Result<StatusCode, AppError> {
    // ポリシー違反は Argon2 の検証より前に弾く (不要な CPU を使わないため)。レートリミットも
    // ここでは消費しない (現在のパスワードを試しておらず、総当たりにも CPU 枯渇にもならない)。
    auth::validate_password(&state.password, &payload.new_password)?;

    // セッションを奪った相手が、現在のパスワードの総当たりや Argon2 の連続実行に使うのを防ぐ。
    let reservation = state
        .password_change_rate_limiter
        .try_acquire(&user.username)
        .ok_or(AppError::TooManyRequests)?;

    let current_hash = auth::load_password_hash(&state.pool, user.id).await?;

    let new_hash = auth::verify_and_hash_password(
        payload.current_password,
        current_hash,
        payload.new_password,
    )
    .await?
    .ok_or(AppError::IncorrectPassword)?;

    // ログインと同じく、正しいパスワードでの変更は上限の消費に数えない。
    state
        .password_change_rate_limiter
        .release(&user.username, reservation);

    let generation =
        auth::update_password_hash(&state.pool, user.id, &new_hash, user.app_token_id).await?;
    tracing::info!(user_id = user.id, "パスワードを変更しました");
    password_notice::spawn(&state, user.id, password_notice::Origin::User);

    // この端末のセッションを新しい世代に載せ替える。失敗してもエラーにはしない: 変更自体は
    // 既に成立しており、500 を返すと利用者は変わっていないと受け取って古いパスワードで
    // やり直し、現在のパスワードの照合のレートリミットを空振りで消費する。載せ替えられ
    // なかった場合に起きるのは、この端末も次のアクセスで断たれて入り直すことだけ。
    // アプリから変えたときは、トークンを上の更新と一緒に載せ替え済み (docs/mobile-app.md)。
    if user.app_token_id.is_none()
        && let Err(err) = session
            .insert(auth::SESSION_GENERATION_KEY, generation)
            .await
    {
        tracing::warn!(
            user_id = user.id,
            error = %crate::error_chain_line(&err),
            "パスワード変更後のセッションの世代の更新に失敗しました"
        );
    }
    Ok(StatusCode::NO_CONTENT)
}

/// 自分の Google・LINE・Apple との連携を解除する (docs/authentication.md)。
///
/// 解除できるのは、解除した後もログインできる方法 (パスワード・ほかの連携) が残るときだけ。
/// LINE・Apple は提供元の側の連携も取り消す。取り消せなくても解除は成立させる (退会時と同じ)。
#[utoipa::path(
    delete,
    path = "/account/identities/{provider}",
    params(("provider" = Provider, Path, description = "連携の提供元")),
    responses(
        (status = 204, description = "解除した"),
        (status = 401, description = "未ログイン", body = ErrorResponse),
        (status = 404, description = "その提供元とは連携していない", body = ErrorResponse),
        (status = 409, description = "解除するとログインできる方法が無くなる", body = ErrorResponse),
    )
)]
async fn unlink_identity(
    user: AuthUser,
    State(state): State<AppState>,
    AppPath(provider): AppPath<String>,
) -> Result<StatusCode, AppError> {
    let provider = Provider::parse(&provider).ok_or(AppError::NotFound)?;
    match oauth_identity::unlink(&state.pool, user.id, provider).await? {
        Unlinked::NotLinked => Err(AppError::NotFound),
        Unlinked::LastLoginMethod => Err(AppError::CannotUnlinkLastLoginMethod),
        Unlinked::Done { revocation_tokens } => {
            tracing::info!(
                user_id = user.id,
                provider = provider.as_str(),
                "外部アカウントとの連携を解除しました"
            );
            if !revocation_tokens.is_empty() {
                crate::external_login::revoke_link_logged(
                    &state.line_login,
                    &state.apple_login,
                    provider,
                    &revocation_tokens,
                    user.id,
                    "提供元との連携を取り消せませんでした。解除は続けます",
                )
                .await;
            }
            Ok(StatusCode::NO_CONTENT)
        }
    }
}

pub fn router() -> OpenApiRouter<AppState> {
    OpenApiRouter::new()
        .routes(routes!(schedule_deletion))
        .routes(routes!(change_password))
        .routes(routes!(unlink_identity))
}
