//! OCR 枠買い足しの決済 (ウェブは Stripe、アプリはアプリ内課金。→ docs/payments.md
//! 「アプリ内課金 (iPhone・iPad)」)。

use axum::Json;
use axum::body::Bytes;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;

use crate::auth::AuthUser;
use crate::error::{AppError, AppJson, AppQuery, ErrorResponse};
use crate::forwarded::ClientIp;
use crate::payments;
use crate::state::AppState;

const STRIPE_SIGNATURE_HEADER: &str = "stripe-signature";

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct OcrTopupCheckoutResponse {
    /// この URL へ遷移すると Stripe-hosted Checkout が開く。
    checkout_url: String,
}

#[utoipa::path(
    post,
    path = "/payments/ocr-topup/checkout",
    responses(
        (status = 200, description = "Checkout Session を作成した", body = OcrTopupCheckoutResponse),
        (status = 401, description = "未ログイン", body = ErrorResponse),
        (status = 502, description = "Stripe API 呼び出しに失敗", body = ErrorResponse),
        (status = 503, description = "Stripe 決済が設定されていない", body = ErrorResponse),
    )
)]
async fn ocr_topup_checkout(
    user: AuthUser,
    State(state): State<AppState>,
) -> Result<Json<OcrTopupCheckoutResponse>, AppError> {
    let customer_email = verified_email(&state, user.id).await?;
    let checkout =
        payments::create_ocr_topup_checkout(&state.stripe, user.id, customer_email.as_deref())
            .await?;
    Ok(Json(OcrTopupCheckoutResponse {
        checkout_url: checkout.checkout_url,
    }))
}

/// メール確認済みのアドレスだけ Checkout の `customer_email` に事前入力する。`users` に専用の
/// email 列は無く、`username` がメールアドレスを兼ねる。
async fn verified_email(state: &AppState, user_id: i64) -> Result<Option<String>, AppError> {
    let username = sqlx::query_scalar!(
        "SELECT username FROM users WHERE id = ? AND email_verified = 1",
        user_id
    )
    .fetch_optional(&state.pool)
    .await?;
    Ok(username.filter(|username| state.mailer.can_send_to(username)))
}

#[derive(Debug, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
#[serde(rename_all = "camelCase")]
pub struct OcrTopupResultQuery {
    /// Checkout Session ID (成功URLの `{CHECKOUT_SESSION_ID}` から)。
    session_id: String,
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct OcrTopupResultResponse {
    status: payments::TopupResultStatus,
}

#[utoipa::path(
    get,
    path = "/payments/ocr-topup/result",
    params(OcrTopupResultQuery),
    responses(
        (status = 200, description = "購入結果 (成功・処理待ち・失敗)", body = OcrTopupResultResponse),
        (status = 401, description = "未ログイン", body = ErrorResponse),
        (status = 404, description = "本人の Session ではない、または存在しない", body = ErrorResponse),
        (status = 502, description = "Stripe API 呼び出しに失敗", body = ErrorResponse),
        (status = 503, description = "Stripe 決済が設定されていない", body = ErrorResponse),
    )
)]
async fn ocr_topup_result(
    user: AuthUser,
    State(state): State<AppState>,
    AppQuery(query): AppQuery<OcrTopupResultQuery>,
) -> Result<Json<OcrTopupResultResponse>, AppError> {
    let status =
        payments::ocr_topup_result(&state.pool, &state.stripe, user.id, &query.session_id).await?;
    Ok(Json(OcrTopupResultResponse { status }))
}

/// Stripe からの Webhook。署名 (`Stripe-Signature`) 検証は生のリクエストボディに対して行うため、
/// JSON 抽出 (`Json<T>`) ではなく `Bytes` で受ける (JSON を経由して再シリアライズすると
/// 署名が一致しなくなる)。
/// ブラウザ由来ではないため認証・CSRF 同一オリジンチェックの対象にしない
/// (`crate::csrf::same_origin_check` は Origin/Referer が無いリクエストを素通りさせる設計)。
#[utoipa::path(
    post,
    path = "/payments/stripe/webhook",
    request_body(content = String, content_type = "application/json", description = "Stripe Event (生の JSON。署名検証のため型付けしない)"),
    responses(
        (status = 200, description = "受理・処理した (未知のイベント種別も含む)"),
        (status = 400, description = "署名が不正", body = ErrorResponse),
        (status = 502, description = "Session の再検証等、Stripe API 呼び出しに失敗", body = ErrorResponse),
        (status = 503, description = "Stripe 決済が設定されていない", body = ErrorResponse),
    )
)]
async fn stripe_webhook(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<StatusCode, AppError> {
    let signature = headers
        .get(STRIPE_SIGNATURE_HEADER)
        .and_then(|value| value.to_str().ok())
        .ok_or(AppError::StripeWebhookInvalidSignature)?;
    payments::handle_stripe_webhook(
        &state.pool,
        &state.stripe,
        state.ocr_topup_grant_milli_yen,
        &body,
        signature,
    )
    .await?;
    Ok(StatusCode::OK)
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct AppleAccountTokenResponse {
    /// 購入オプションの `appAccountToken` に渡す UUID (小文字)。アカウントごとに固定。
    app_account_token: String,
}

#[utoipa::path(
    post,
    path = "/payments/apple/account-token",
    responses(
        (status = 200, description = "アカウントの appAccountToken (無ければ作った)", body = AppleAccountTokenResponse),
        (status = 401, description = "未ログイン", body = ErrorResponse),
        (status = 503, description = "アプリ内課金が設定されていない", body = ErrorResponse),
    )
)]
async fn apple_account_token(
    user: AuthUser,
    State(state): State<AppState>,
) -> Result<Json<AppleAccountTokenResponse>, AppError> {
    if !state.app_store.enabled() {
        return Err(AppError::PaymentsDisabled);
    }
    let app_account_token = payments::apple_app_account_token(&state.pool, user.id).await?;
    Ok(Json(AppleAccountTokenResponse { app_account_token }))
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct AppleTransactionRequest {
    /// StoreKit の `Transaction.id`。
    transaction_id: String,
    /// StoreKit の `Transaction.environment` の値 (`Production`・`Sandbox`・`Xcode`)。先にこの環境の API から引く。
    /// `Xcode` (StoreKit のローカルのテスト) の取引は Apple に無いので `rejected` になる。
    environment: String,
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct AppleTransactionResponse {
    /// どれでもアプリは取引を終える (`finish()`)。エラーの応答なら終えない。
    status: payments::AppleTransactionStatus,
}

/// 枠は取引の `appAccountToken` の持ち主に付く (送ってきたアカウントとは限らない)。
#[utoipa::path(
    post,
    path = "/payments/apple/transactions",
    request_body = AppleTransactionRequest,
    responses(
        (status = 200, description = "取引を Apple から取り直して反映した", body = AppleTransactionResponse),
        (status = 401, description = "未ログイン", body = ErrorResponse),
        (status = 429, description = "送る回数の上限を超えた (アプリは取引を終えず、次の起動で送り直す)", body = ErrorResponse),
        (status = 503, description = "アプリ内課金が設定されていない (payments_disabled)、または今は確かめられない (app_store_unavailable)", body = ErrorResponse),
    )
)]
async fn apple_transaction(
    user: AuthUser,
    State(state): State<AppState>,
    AppJson(request): AppJson<AppleTransactionRequest>,
) -> Result<Json<AppleTransactionResponse>, AppError> {
    state
        .app_store_transaction_rate_limiter
        .try_acquire(&user.id.to_string())
        .ok_or(AppError::TooManyRequests)?;
    let status = payments::submit_apple_transaction(
        &state.pool,
        &state.app_store,
        state.ocr_topup_grant_milli_yen,
        &request.transaction_id,
        &request.environment,
    )
    .await?;
    Ok(Json(AppleTransactionResponse { status }))
}

/// App Store Server Notifications V2。署名は確かめず、取引を Apple から取り直して反映する。
/// 認証・CSRF の対象にしない理由は `stripe_webhook` と同じ。
#[utoipa::path(
    post,
    path = "/payments/apple/notifications",
    request_body(content = String, content_type = "application/json", description = "responseBodyV2 (`signedPayload`)"),
    responses(
        (status = 200, description = "反映した、または関係の無い通知"),
        (status = 400, description = "形が不正", body = ErrorResponse),
        (status = 403, description = "送り元が Apple の範囲ではない", body = ErrorResponse),
        (status = 503, description = "アプリ内課金が設定されていない、または今は確かめられない (再送させる)", body = ErrorResponse),
    )
)]
async fn apple_notification(
    State(state): State<AppState>,
    ClientIp(client_ip): ClientIp,
    body: Bytes,
) -> Result<StatusCode, AppError> {
    payments::handle_apple_notification(
        &state.pool,
        &state.app_store,
        state.ocr_topup_grant_milli_yen,
        client_ip,
        &body,
    )
    .await?;
    Ok(StatusCode::OK)
}

pub fn router() -> OpenApiRouter<AppState> {
    OpenApiRouter::new()
        .routes(routes!(ocr_topup_checkout))
        .routes(routes!(ocr_topup_result))
        .routes(routes!(stripe_webhook))
        .routes(routes!(apple_account_token))
        .routes(routes!(apple_transaction))
        .routes(routes!(apple_notification))
}
