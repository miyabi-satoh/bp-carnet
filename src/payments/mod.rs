//! 決済と購入した OCR 枠の境界。Checkout の作成・Webhook 処理と、アプリ内課金の取引・通知の処理の
//! アプリケーションサービスをここに置き、Stripe・Apple 固有の HTTP 形式・型は `stripe`・`app_store`
//! サブモジュールに閉じる。

pub mod app_store;
pub mod ocr_quota;
pub mod stripe;

use serde::Serialize;
use sqlx::{Connection, Executor, Sqlite, SqlitePool};
use utoipa::ToSchema;

use crate::error::AppError;
use crate::payments::app_store::{AppStoreClient, AppStoreError, Notification};
use crate::payments::ocr_quota::AppStorePurchaseChange;
use crate::payments::stripe::{Charge, CheckoutSession, StripeClient, StripeError, WebhookEvent};

impl From<StripeError> for AppError {
    fn from(err: StripeError) -> Self {
        match err {
            StripeError::Disabled => AppError::PaymentsDisabled,
            StripeError::TaxRateMissing => {
                tracing::error!(
                    "課税事業者になった後なのに STRIPE_TAX_RATE_ID が無いため、Checkout を作りません"
                );
                AppError::PaymentsDisabled
            }
            StripeError::InvalidSignature | StripeError::SignatureTimestampOutOfRange => {
                AppError::StripeWebhookInvalidSignature
            }
            StripeError::Request(_)
            | StripeError::UpstreamStatus(_)
            | StripeError::ParseResponse => AppError::PaymentsUpstream {
                message: err.to_string(),
                cause: Some(err),
            },
        }
    }
}

pub(crate) fn now_sql() -> String {
    format_sql(jiff::Timestamp::now())
}

/// DB に書く日時の書式 (UTC、ミリ秒まで)。文字列のまま大小を比べるので、SQL 側で作る日時
/// (`strftime('%Y-%m-%dT%H:%M:%fZ', ...)`) と同じ書式にする。
pub(crate) fn format_sql(timestamp: jiff::Timestamp) -> String {
    timestamp.strftime("%Y-%m-%dT%H:%M:%S%.3fZ").to_string()
}

/// `POST /payments/ocr-topup/checkout` の結果。
pub struct CheckoutStatus {
    pub checkout_url: String,
}

/// サーバー設定の Price ID だけで Checkout Session を作る。クライアントから金額・Price ID・
/// 対象ユーザーを受け取らない。
pub async fn create_ocr_topup_checkout(
    stripe: &StripeClient,
    user_id: i64,
    customer_email: Option<&str>,
) -> Result<CheckoutStatus, AppError> {
    if !stripe.enabled() {
        return Err(AppError::PaymentsDisabled);
    }
    let session = stripe
        .create_checkout_session(user_id, customer_email)
        .await?;
    let checkout_url = session.url.ok_or(AppError::PaymentsUpstream {
        message: "checkout session response is missing url".to_string(),
        cause: None,
    })?;
    Ok(CheckoutStatus { checkout_url })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
#[schema(as = OcrTopupResultStatus)]
pub enum TopupResultStatus {
    /// Checkout は完了しているが、Webhook の処理待ち (または未確定)。
    Processing,
    /// 購入枠が付与済み。
    Succeeded,
    /// 支払いが失敗・期限切れ。
    Failed,
}

/// 購入結果を確認する。DB (grant の有無) で判定できればそれを優先し、Stripe API は
/// 「まだ Webhook が処理していない」場合と、本人の Session かどうかの検証にだけ使う。
pub async fn ocr_topup_result(
    pool: &SqlitePool,
    stripe: &StripeClient,
    user_id: i64,
    session_id: &str,
) -> Result<TopupResultStatus, AppError> {
    if !stripe.enabled() {
        return Err(AppError::PaymentsDisabled);
    }
    if ocr_quota::grant_exists(pool, user_id, session_id).await? {
        // 付与後に charge.refunded・charge.dispute.created が届き、枠を失効させていれば
        // 成功表示のままにしない。
        if webhook_event_checkout_session_was_revoked(pool, session_id).await? {
            return Ok(TopupResultStatus::Failed);
        }
        return Ok(TopupResultStatus::Succeeded);
    }

    let session = stripe.retrieve_checkout_session(session_id).await?;
    if session.metadata.get("user_id").map(String::as_str) != Some(user_id.to_string().as_str()) {
        // 他人の Session ID を渡された。存在を明かさない。
        return Err(AppError::NotFound);
    }
    if session.status.as_str() == "expired"
        || webhook_event_checkout_session_async_payment_failed(pool, session_id).await?
        // 配送順序の逆転 (charge.refunded・charge.dispute.created が completed より先) で
        // grantがそもそも作られなかった場合、ここで拾わないと processing のまま止まる。
        || webhook_event_checkout_session_was_revoked(pool, session_id).await?
    {
        return Ok(TopupResultStatus::Failed);
    }
    Ok(TopupResultStatus::Processing)
}

/// Webhook を検証・冪等処理する。署名検証済みの生バイト列を受け取る (呼び出し側の
/// `api::payments` が axum の raw body から渡す)。`grant_milli_yen` は、決済の完了で付ける量。
pub async fn handle_stripe_webhook(
    pool: &SqlitePool,
    stripe: &StripeClient,
    grant_milli_yen: i64,
    payload: &[u8],
    signature_header: &str,
) -> Result<(), AppError> {
    if !stripe.enabled() {
        return Err(AppError::PaymentsDisabled);
    }
    stripe.verify_webhook_signature(payload, signature_header)?;

    let event: WebhookEvent =
        serde_json::from_slice(payload).map_err(|_| AppError::InvalidJson {
            status: axum::http::StatusCode::BAD_REQUEST,
            message: "invalid stripe event payload".to_string(),
        })?;

    // 冪等性: 処理済みのイベントはここで打ち切る。'received'・'failed' のまま止まっている
    // (途中失敗の) イベントは、再送を受けて処理をやり直せるようにする。
    if webhook_event_status(pool, &event.id).await?.as_deref() == Some("processed") {
        return Ok(());
    }
    webhook_event_upsert_received(pool, &event.id, &event.event_type).await?;

    let result = dispatch_stripe_event(pool, stripe, grant_milli_yen, &event).await;
    match &result {
        Ok(checkout_session_id) => {
            webhook_event_mark_processed(pool, &event.id, checkout_session_id.as_deref()).await?
        }
        Err(err) => {
            webhook_event_mark_failed(pool, &event.id, &crate::error_chain_line(err)).await?;
        }
    }
    result.map(|_| ())
}

/// 処理結果とあわせて、関連する Checkout Session ID (分かる場合) を返す。
/// `stripe_webhook_events.checkout_session_id` に記録し、`charge.refunded`・
/// `charge.dispute.created` が `checkout.session.completed` より先に届いた場合の
/// 判定 (`was_revoked`) に使う。
async fn dispatch_stripe_event(
    pool: &SqlitePool,
    stripe: &StripeClient,
    grant_milli_yen: i64,
    event: &WebhookEvent,
) -> Result<Option<String>, AppError> {
    match event.event_type.as_str() {
        "checkout.session.completed" | "checkout.session.async_payment_succeeded" => {
            let session: CheckoutSession = serde_json::from_value(event.data.object.clone())
                .map_err(|_| invalid_event_object())?;
            grant_ocr_topup(pool, stripe, grant_milli_yen, &session, event.created).await?;
            Ok(Some(session.id))
        }
        "checkout.session.async_payment_failed" => {
            // 枠は付与しないが、Session IDを記録して `/payments/ocr-topup/result` が
            // 失敗と判定できるようにする (コンビニ決済などの非同期手段は、失敗しても
            // Checkout Session がすぐには `expired` にならないことがある)。
            let session: CheckoutSession = serde_json::from_value(event.data.object.clone())
                .map_err(|_| invalid_event_object())?;
            Ok(Some(session.id))
        }
        "charge.refunded" | "charge.dispute.created" => {
            let charge: Charge = serde_json::from_value(event.data.object.clone())
                .map_err(|_| invalid_event_object())?;
            revoke_by_payment_intent(pool, stripe, charge.payment_intent.as_deref(), &event.id)
                .await
        }
        _ => {
            // 未知のイベントは記録して2xxを返す。
            Ok(None)
        }
    }
}

fn invalid_event_object() -> AppError {
    AppError::PaymentsUpstream {
        message: "unexpected shape for the stripe event object".to_string(),
        cause: None,
    }
}

/// 支払い確定を検証し、枠を付与する。`event_created_unix` はイベント生成時刻 (Webhook配送・
/// 処理の遅延を持ち込まないよう、`purchased_at` にこちらを使う)。
async fn grant_ocr_topup(
    pool: &SqlitePool,
    stripe: &StripeClient,
    grant_milli_yen: i64,
    session: &CheckoutSession,
    event_created_unix: i64,
) -> Result<(), AppError> {
    if session.mode != "payment" || session.payment_status != "paid" {
        return Ok(());
    }
    // 同じアカウントのほかの製品の決済は、何もせずに受け取る。印が無いのは、印を付ける前に
    // 売ったこのアプリの Session なので、下の検証に進める。
    if session
        .metadata
        .get("product")
        .is_some_and(|product| product != stripe::PRODUCT)
    {
        return Ok(());
    }
    // metadata だけでなく Stripe API から取得し直したものを検証する。
    let verified = stripe.retrieve_checkout_session(&session.id).await?;
    if verified.payment_status != "paid" {
        return Ok(());
    }
    if verified.amount_total != Some(ocr_quota::TOPUP_PRICE_YEN)
        || verified.currency.as_deref() != Some("jpy")
    {
        // サーバーが作成したもの以外 (想定と違う額・通貨) からのイベントの疑い。
        // 想定額と食い違うため、人が確認するまで付与しない。
        tracing::error!(
            checkout_session_id = %session.id,
            amount_total = ?verified.amount_total,
            currency = ?verified.currency,
            "想定 ({}円) と異なる決済のためOCR枠を付与しません",
            ocr_quota::TOPUP_PRICE_YEN
        );
        return Ok(());
    }
    // line item が1件だけであることも確認する (先頭要素だけを見ると、2件目以降に
    // 別の Price を混ぜられても検出できない)。
    let matches_configured_price = verified.line_items.as_ref().is_some_and(|list| {
        list.data.len() == 1
            && list.data.first().is_some_and(|item| {
                item.quantity == Some(1)
                    && item.price.as_ref().map(|p| p.id.as_str()) == stripe.price_id()
            })
    });
    if !matches_configured_price {
        // サーバーが作成したもの以外 (想定と違うPrice・数量) からのイベントの疑い。
        tracing::error!(
            checkout_session_id = %session.id,
            "想定と異なるPrice・数量のためOCR枠を付与しません"
        );
        return Ok(());
    }
    let Some(user_id) = verified
        .metadata
        .get("user_id")
        .and_then(|value| value.parse::<i64>().ok())
    else {
        tracing::error!(
            checkout_session_id = %session.id,
            "Checkout Session に user_id metadata がありません"
        );
        return Ok(());
    };

    let purchased_at =
        jiff::Timestamp::from_second(event_created_unix).map_err(|_| invalid_event_object())?;

    // `charge.refunded`・`charge.dispute.created` が先に届いていた (配送順序の逆転) 場合に、
    // 返金・異議申立て済みの決済へ付与しないよう、確認と付与を1つの排他的なトランザクション
    // (`BEGIN IMMEDIATE`) にする。通常の `BEGIN` (deferred) だと、確認と付与の間に別の
    // トランザクションが割り込める。
    let mut conn = pool.acquire().await?;
    let mut tx = conn.begin_with("BEGIN IMMEDIATE").await?;
    if webhook_event_checkout_session_was_revoked(&mut *tx, &session.id).await? {
        tracing::warn!(
            checkout_session_id = %session.id,
            "返金・異議申立て済みのCheckout Sessionのため枠を付与しません"
        );
        return Ok(());
    }
    let domestic = verified.buyer_is_domestic();
    if domestic.is_none() {
        // 申告のときに、Stripe の明細で国を確かめる (docs/payments.md)。
        tracing::warn!(checkout_session_id = %session.id, "Checkout Session に住所の国がありません");
    }
    ocr_quota::grant(
        &mut tx,
        user_id,
        &session.id,
        &format_sql(purchased_at),
        grant_milli_yen,
        domestic,
    )
    .await?;
    tx.commit().await?;
    Ok(())
}

async fn revoke_by_payment_intent(
    pool: &SqlitePool,
    stripe: &StripeClient,
    payment_intent_id: Option<&str>,
    event_id: &str,
) -> Result<Option<String>, AppError> {
    let Some(payment_intent_id) = payment_intent_id else {
        return Ok(None);
    };
    let Some(session) = stripe
        .find_checkout_session_by_payment_intent(payment_intent_id)
        .await?
    else {
        return Ok(None);
    };
    // grant側 (`grant_ocr_topup`) の `was_revoked` 判定と同じ排他的トランザクション
    // (`BEGIN IMMEDIATE`) の中で、枠の失効と「処理済み」記録を一緒にコミットする。
    // 別トランザクションに分けると、失効のコミット後・記録の書き込み前の隙間に
    // grant側の判定が割り込み、返金済みなのに枠を付与してしまう競合状態が残る。
    let mut conn = pool.acquire().await?;
    let mut tx = conn.begin_with("BEGIN IMMEDIATE").await?;
    ocr_quota::revoke_unspent(&mut tx, &session.id).await?;
    webhook_event_mark_processed(&mut *tx, event_id, Some(&session.id)).await?;
    tx.commit().await?;
    Ok(Some(session.id))
}

async fn webhook_event_status(
    pool: &SqlitePool,
    event_id: &str,
) -> Result<Option<String>, AppError> {
    sqlx::query_scalar!(
        "SELECT status FROM stripe_webhook_events WHERE event_id = ?",
        event_id
    )
    .fetch_optional(pool)
    .await
    .map_err(AppError::from)
}

async fn webhook_event_upsert_received(
    pool: &SqlitePool,
    event_id: &str,
    event_type: &str,
) -> Result<(), AppError> {
    let now = now_sql();
    sqlx::query!(
        "INSERT INTO stripe_webhook_events (event_id, status, event_type, received_at) \
         VALUES (?, 'received', ?, ?) \
         ON CONFLICT(event_id) DO UPDATE SET status = 'received', event_type = excluded.event_type",
        event_id,
        event_type,
        now
    )
    .execute(pool)
    .await?;
    Ok(())
}

async fn webhook_event_mark_processed(
    executor: impl Executor<'_, Database = Sqlite>,
    event_id: &str,
    checkout_session_id: Option<&str>,
) -> Result<(), AppError> {
    let now = now_sql();
    sqlx::query!(
        "UPDATE stripe_webhook_events \
         SET status = 'processed', processed_at = ?, \
             checkout_session_id = COALESCE(?, checkout_session_id) \
         WHERE event_id = ?",
        now,
        checkout_session_id,
        event_id
    )
    .execute(executor)
    .await?;
    Ok(())
}

/// `checkout.session.async_payment_failed` が同じ Checkout Session に対して既に処理済みか
/// (コンビニ決済などの非同期手段の失敗を、`/payments/ocr-topup/result` が
/// `Processing` のまま止めずに `Failed` として返すために使う)。
async fn webhook_event_checkout_session_async_payment_failed(
    pool: &SqlitePool,
    checkout_session_id: &str,
) -> Result<bool, AppError> {
    let found = sqlx::query_scalar!(
        r#"SELECT 1 AS "value!: i64" FROM stripe_webhook_events
         WHERE checkout_session_id = ? AND status = 'processed'
           AND event_type = 'checkout.session.async_payment_failed'
         LIMIT 1"#,
        checkout_session_id
    )
    .fetch_optional(pool)
    .await?;
    Ok(found.is_some())
}

/// `charge.refunded`・`charge.dispute.created` が同じ Checkout Session に対して既に処理済みか
/// (配送順序が `checkout.session.completed` より先だった場合の判定)。
async fn webhook_event_checkout_session_was_revoked(
    executor: impl Executor<'_, Database = Sqlite>,
    checkout_session_id: &str,
) -> Result<bool, AppError> {
    let found = sqlx::query_scalar!(
        r#"SELECT 1 AS "value!: i64" FROM stripe_webhook_events
         WHERE checkout_session_id = ? AND status = 'processed'
           AND event_type IN ('charge.refunded', 'charge.dispute.created')
         LIMIT 1"#,
        checkout_session_id
    )
    .fetch_optional(executor)
    .await?;
    Ok(found.is_some())
}

async fn webhook_event_mark_failed(
    pool: &SqlitePool,
    event_id: &str,
    reason: &str,
) -> Result<(), AppError> {
    let now = now_sql();
    sqlx::query!(
        "UPDATE stripe_webhook_events SET status = 'failed', processed_at = ?, failure_reason = ? WHERE event_id = ?",
        now,
        reason,
        event_id
    )
    .execute(pool)
    .await?;
    Ok(())
}

// ---- アプリ内課金 (docs/payments.md) ----

/// アカウントの appAccountToken (UUID、小文字)。無ければ作る。2台から同時に作っても1つに決まるよう、
/// 空のときだけ書いてから読み直す。
pub async fn apple_app_account_token(pool: &SqlitePool, user_id: i64) -> Result<String, AppError> {
    let candidate = crate::token::random_uuid_v4();
    sqlx::query!(
        "UPDATE users SET apple_app_account_token = ? WHERE id = ? AND apple_app_account_token IS NULL",
        candidate,
        user_id
    )
    .execute(pool)
    .await?;
    sqlx::query_scalar!(
        r#"SELECT apple_app_account_token AS "token!: String" FROM users WHERE id = ?"#,
        user_id
    )
    .fetch_optional(pool)
    .await?
    .ok_or(AppError::Unauthorized)
}

/// アプリから届いた取引を、Apple から取り直して枠に反映した結果。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
#[schema(as = AppStoreTransactionStatus)]
pub enum AppleTransactionStatus {
    /// 枠を足した (取り消しが外れて戻した場合も含む)。
    Granted,
    /// 足し済み。
    AlreadyGranted,
    /// ずっと足さない (商品・持ち主が合わない、取り消し済み)。
    Rejected,
}

impl From<AppStoreError> for AppError {
    fn from(err: AppStoreError) -> Self {
        match err {
            AppStoreError::Disabled => AppError::PaymentsDisabled,
            _ => AppError::AppStoreUnavailable(err),
        }
    }
}

/// `POST /payments/apple/transactions`。アプリが添えた環境 (`Transaction.environment`) の API から引き、
/// 無ければもう一方でも引く。`Production`・`Sandbox` 以外の環境 (StoreKit のローカルのテストの `Xcode`)
/// の取引は Apple に無いので、引かずに足さないと決める。`grant_milli_yen` は、新しい取引に付ける量。
pub async fn submit_apple_transaction(
    pool: &SqlitePool,
    app_store: &AppStoreClient,
    grant_milli_yen: i64,
    transaction_id: &str,
    environment: &str,
) -> Result<AppleTransactionStatus, AppError> {
    if !app_store.enabled() {
        return Err(AppError::PaymentsDisabled);
    }
    let Some(environment) = app_store::Environment::parse(environment) else {
        tracing::warn!(
            transaction_id,
            environment,
            "App Store の外の環境の取引のため、枠を足しません"
        );
        return Ok(AppleTransactionStatus::Rejected);
    };
    let transaction = match app_store
        .find_transaction(transaction_id, environment)
        .await
    {
        Ok(transaction) => transaction,
        Err(AppStoreError::Unusable(reason)) => {
            tracing::error!(
                transaction_id,
                reason,
                "取引として使えないため、枠を足しません"
            );
            return Ok(AppleTransactionStatus::Rejected);
        }
        Err(err) => return Err(err.into()),
    };
    apply_apple_transaction(pool, app_store, grant_milli_yen, &transaction).await
}

/// `POST /payments/apple/notifications`。送り元が Apple の範囲でないものは `AppError::Forbidden`、
/// 形が不正なものは 400 にする。関係の無い通知は何もせずに受け取る。`grant_milli_yen` は、新しい取引に付ける量。
pub async fn handle_apple_notification(
    pool: &SqlitePool,
    app_store: &AppStoreClient,
    grant_milli_yen: i64,
    client_ip: Option<std::net::IpAddr>,
    body: &[u8],
) -> Result<(), AppError> {
    if !app_store.enabled() {
        return Err(AppError::PaymentsDisabled);
    }
    if !app_store.is_notification_source(client_ip) {
        tracing::warn!(client_ip = ?client_ip, "Apple の範囲の外から届いた通知を捨てます");
        return Err(AppError::Forbidden);
    }
    let target = match app_store.read_notification(body) {
        Some(Notification::Transaction(target)) => target,
        Some(Notification::Ignored) => return Ok(()),
        None => {
            return Err(AppError::InvalidJson {
                status: axum::http::StatusCode::BAD_REQUEST,
                message: "invalid app store notification".to_string(),
            });
        }
    };
    let transaction = match app_store
        .get_transaction(&target.transaction_id, target.environment)
        .await
    {
        Ok(Some(transaction)) => transaction,
        // 見つからないのはまれに一時的に起きるので、再送させる。
        Ok(None) => {
            return Err(AppError::AppStoreUnavailable(AppStoreError::Temporary(
                "transaction not found",
            )));
        }
        Err(AppStoreError::Unusable(reason)) => {
            tracing::error!(transaction_id = %target.transaction_id, reason, "通知の取引が取引として使えないため、捨てます");
            return Ok(());
        }
        Err(err) => return Err(err.into()),
    };
    let status = apply_apple_transaction(pool, app_store, grant_milli_yen, &transaction).await?;
    tracing::info!(
        notification_type = %target.notification_type,
        transaction_id = %transaction.transaction_id,
        environment = transaction.environment.as_str(),
        // 取り消した取引も `Rejected` (もう足さない) になるので、返金の反映かが分かるように添える。
        revoked = transaction.is_revoked(),
        status = ?status,
        "App Store の通知を反映しました"
    );
    Ok(())
}

/// 取り直した取引を、今の行の状態に合わせて1つだけ反映する。同じ状態のままなら何もしない
/// (再送・重複・順序の前後で結果が変わらない)。足すのと消すのは同じ `BEGIN IMMEDIATE` の中で行う。
async fn apply_apple_transaction(
    pool: &SqlitePool,
    app_store: &AppStoreClient,
    grant_milli_yen: i64,
    transaction: &app_store::Transaction,
) -> Result<AppleTransactionStatus, AppError> {
    let bundle_id = app_store.bundle_id().ok_or(AppError::PaymentsDisabled)?;
    if !app_store::is_topup_product(transaction, bundle_id) {
        tracing::error!(
            transaction_id = %transaction.transaction_id,
            product_id = %transaction.product_id,
            kind = %transaction.kind,
            quantity = ?transaction.quantity,
            "買い足しの商品と合わない取引のため、枠を足しません"
        );
        return Ok(AppleTransactionStatus::Rejected);
    }
    let token = transaction
        .app_account_token
        .as_deref()
        .map(str::to_ascii_lowercase);
    let owner = match token {
        Some(token) => {
            sqlx::query_scalar!(
                r#"SELECT id AS "id!: i64" FROM users WHERE apple_app_account_token = ?"#,
                token
            )
            .fetch_optional(pool)
            .await?
        }
        None => None,
    };
    let Some(owner) = owner else {
        // アカウントを消した後に返金や返金の取り消しが届いたら、残した購入の記録の印だけを合わせる。
        if ocr_quota::sync_orphaned_app_store_purchase(
            pool,
            &transaction.transaction_id,
            transaction.is_revoked(),
            transaction.signed_date,
        )
        .await?
        {
            tracing::info!(
                transaction_id = %transaction.transaction_id,
                revoked = transaction.is_revoked(),
                "削除したアカウントの購入の記録に、取引の状態を合わせました"
            );
            return Ok(AppleTransactionStatus::Rejected);
        }
        tracing::error!(
            transaction_id = %transaction.transaction_id,
            "appAccountToken の持ち主がいない取引のため、枠を足しません"
        );
        return Ok(AppleTransactionStatus::Rejected);
    };
    let Ok(purchased_at) = jiff::Timestamp::from_millisecond(transaction.purchase_date) else {
        tracing::error!(
            transaction_id = %transaction.transaction_id,
            purchase_date = transaction.purchase_date,
            "購入日時が読めない取引のため、枠を足しません"
        );
        return Ok(AppleTransactionStatus::Rejected);
    };

    let mut conn = pool.acquire().await?;
    let mut tx = conn.begin_with("BEGIN IMMEDIATE").await?;
    let change = ocr_quota::apply_app_store_purchase(
        &mut tx,
        &ocr_quota::AppStorePurchase {
            user_id: owner,
            transaction_id: &transaction.transaction_id,
            environment: transaction.environment.as_str(),
            purchased_at: &format_sql(purchased_at),
            revoked: transaction.is_revoked(),
            signed_date: transaction.signed_date,
            domestic: transaction.buyer_is_domestic(),
            granted_milli_yen: grant_milli_yen,
        },
    )
    .await?;
    tx.commit().await?;
    Ok(match change {
        AppStorePurchaseChange::Granted | AppStorePurchaseChange::Restored => {
            AppleTransactionStatus::Granted
        }
        AppStorePurchaseChange::AlreadyActive => AppleTransactionStatus::AlreadyGranted,
        AppStorePurchaseChange::Revoked | AppStorePurchaseChange::AlreadyRevoked => {
            AppleTransactionStatus::Rejected
        }
    })
}
