//! 血圧計の液晶を撮影した写真から血圧値を読み取る OCR (Gemini 連携)。

use axum::Json;
use axum::extract::multipart::MultipartError;
use axum::extract::{DefaultBodyLimit, State};
use axum::http::StatusCode;
use serde::Serialize;
use utoipa::ToSchema;
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;

use crate::auth::AuthUser;
use crate::error::{AppError, AppMultipart, ErrorResponse};
use crate::ocr::{OcrError, OcrResult, OcrUsage};
use crate::ocr_consent;
use crate::ocr_usage;
use crate::state::AppState;

/// アップロード画像の上限。クライアント側で ~1600px にリサイズする前提だが、
/// リサイズをすり抜けた大きな画像を早期に弾くための保険 (PoC を踏襲)。
const OCR_MAX_UPLOAD_BYTES: usize = 10 * 1024 * 1024;

/// axum の `DefaultBodyLimit` に渡す上限。`multipart/form-data` はフィールド名や
/// 境界文字列などのオーバーヘッドを画像本体に上乗せするため、`OCR_MAX_UPLOAD_BYTES`
/// ちょうどに設定すると、画像自体はギリギリ上限内のリクエストまでこのレイヤーで
/// 弾かれてしまう。余裕を持たせ、実際の上限判定はハンドラ内の明示チェックに委ねる
/// (このレイヤーで弾かれた場合も `multipart_error_to_app_error` で `ocr_image_too_large`
/// に変換されるため、どちらの経路でもクライアントから見た応答は同じになる)。
const OCR_BODY_LIMIT_BYTES: usize = OCR_MAX_UPLOAD_BYTES + 64 * 1024;

/// 全部の枠を足した残りが、買い足し1回分のこの割合 (%) 以下なら残りが少ないとする。
const QUOTA_LOW_PERCENT: i64 = 20;

/// Gemini に送ってよい画像形式のみ許可する。
const ALLOWED_MIME_TYPES: &[&str] = &["image/jpeg", "image/png", "image/webp"];

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct OcrStatusResponse {
    /// `false` の場合、フロントエンドは OCR 機能自体を隠す。
    enabled: bool,
    /// 読み取りの枠を使う順 (購入の枠を買った順、無料の枠は最後) に並べたもの。
    /// 無料の枠が無制限なら `null` (画面には出さない、docs/ocr.md)。空なら使える枠が無い。
    #[schema(required = true)]
    quotas: Option<Vec<OcrQuotaResponse>>,
    /// 全部の枠を足した残りが、買い足し1回分の20%以下か (使い切りを含む。無制限なら `false`)。
    quota_low: bool,
    /// 読み取りを買い足せるか。Stripe 決済が有効で、無料の枠に上限があるとき `true`。
    topup_available: bool,
    /// アプリで、アプリ内課金で読み取りを買い足せるか。App Store Server API の鍵の設定があり、
    /// 無料の枠に上限があるとき `true`。アプリは `topupAvailable` でなくこちらを見る。
    app_store_topup_available: bool,
    /// 写真を Gemini API に送ることに同意しているか。`false` なら、画面は読み取る前に同意を求める。
    consented: bool,
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum OcrQuotaKind {
    Paid,
    Free,
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct OcrQuotaResponse {
    kind: OcrQuotaKind,
    /// 購入日時 (UTC、RFC3339)。無料の枠は `null`。
    #[schema(required = true, example = "2026-09-27T00:00:00.000Z")]
    purchased_at: Option<String>,
    /// 残りの割合 (%)。0〜100 の整数で、使い切りだけが 0。
    remaining_percent: i64,
}

impl From<&ocr_usage::Quota> for OcrQuotaResponse {
    fn from(quota: &ocr_usage::Quota) -> Self {
        let (kind, purchased_at) = match &quota.kind {
            ocr_usage::QuotaKind::Paid { purchased_at } => {
                (OcrQuotaKind::Paid, Some(purchased_at.clone()))
            }
            ocr_usage::QuotaKind::Free => (OcrQuotaKind::Free, None),
        };
        Self {
            kind,
            purchased_at,
            remaining_percent: quota.remaining_percent(),
        }
    }
}

/// OpenAPI ドキュメント用のダミー型 (実際の抽出は `AppMultipart` で行う)。
/// utoipa-axum はハンドラ引数から `multipart/form-data` を認識できないため、
/// `request_body` として明示的に指定する。
#[derive(ToSchema)]
#[allow(dead_code)]
struct OcrUploadForm {
    #[schema(value_type = String, format = Binary)]
    image: Vec<u8>,
}

#[utoipa::path(
    get,
    path = "/ocr/status",
    responses(
        (status = 200, description = "OCR 機能が利用可能かどうか", body = OcrStatusResponse),
        (status = 401, description = "未ログイン", body = ErrorResponse),
    )
)]
async fn ocr_status(
    user: AuthUser,
    State(state): State<AppState>,
) -> Result<Json<OcrStatusResponse>, AppError> {
    let quotas = ocr_usage::quotas(&state.pool, user.id, state.ocr_free_budget_yen).await?;
    let quota_low = quotas.as_ref().is_some_and(|quotas| {
        let total: i64 = quotas.iter().map(|quota| quota.remaining_milli_yen).sum();
        i128::from(total) * 100
            <= i128::from(state.ocr_topup_grant_milli_yen) * i128::from(QUOTA_LOW_PERCENT)
    });
    Ok(Json(OcrStatusResponse {
        enabled: state.ocr.enabled(),
        topup_available: state.stripe.enabled() && quotas.is_some(),
        app_store_topup_available: state.app_store.enabled() && quotas.is_some(),
        quotas: quotas.map(|quotas| quotas.iter().map(OcrQuotaResponse::from).collect()),
        quota_low,
        consented: ocr_consent::is_consented(&state.pool, user.id).await?,
    }))
}

#[utoipa::path(
    post,
    path = "/ocr",
    request_body(content = OcrUploadForm, content_type = "multipart/form-data"),
    responses(
        (status = 200, description = "読み取り結果 (血圧値が読めなかった場合も200)", body = OcrResult),
        (status = 400, description = "multipart ボディが不正、または image フィールドが無い", body = ErrorResponse),
        (status = 401, description = "未ログイン", body = ErrorResponse),
        (status = 403, description = "写真を Gemini API に送ることに同意していない (`ocr_consent_required`)", body = ErrorResponse),
        (status = 409, description = "同じユーザーの読み取りがまだ終わっていない (`ocr_in_progress`)", body = ErrorResponse),
        (status = 413, description = "画像が10MBを超える", body = ErrorResponse),
        (status = 422, description = "画像のMIMEタイプが対応外 (jpeg/png/webp以外)", body = ErrorResponse),
        (status = 429, description = "全部の読み取りの枠を使い切った (`ocr_budget_exhausted`)", body = ErrorResponse),
        (status = 502, description = "Gemini の呼び出し、またはレスポンスの解釈に失敗", body = ErrorResponse),
        (status = 503, description = "GEMINI_API_KEY 未設定でOCR機能が無効", body = ErrorResponse),
    )
)]
async fn ocr_extract(
    user: AuthUser,
    State(state): State<AppState>,
    AppMultipart(mut multipart): AppMultipart,
) -> Result<Json<OcrResult>, AppError> {
    if !state.ocr.enabled() {
        return Err(AppError::OcrDisabled);
    }
    // 写真を受け取る前に確かめる (同意の無い写真は、サーバーのメモリにも置かない)。
    ocr_consent::ensure_consented(&state.pool, user.id).await?;

    let mut image_bytes: Option<Vec<u8>> = None;
    let mut mime_type: Option<String> = None;

    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(multipart_error_to_app_error)?
    {
        if field.name() != Some("image") {
            continue;
        }
        mime_type = field.content_type().map(str::to_string);
        let bytes = field.bytes().await.map_err(multipart_error_to_app_error)?;
        image_bytes = Some(bytes.to_vec());
        break;
    }

    let image_bytes = image_bytes.ok_or_else(|| AppError::InvalidMultipart {
        status: axum::http::StatusCode::BAD_REQUEST,
        message: "image field is required".to_string(),
    })?;
    if image_bytes.is_empty() {
        return Err(AppError::InvalidMultipart {
            status: axum::http::StatusCode::BAD_REQUEST,
            message: "image is empty".to_string(),
        });
    }
    if image_bytes.len() > OCR_MAX_UPLOAD_BYTES {
        return Err(AppError::OcrImageTooLarge);
    }
    let mime_type = mime_type.unwrap_or_else(|| "image/jpeg".to_string());
    if !ALLOWED_MIME_TYPES.contains(&mime_type.as_str()) {
        return Err(AppError::OcrUnsupportedMimeType(format!(
            "unsupported image type: {mime_type} (expected jpeg, png, or webp)"
        )));
    }

    // 開発用: 同じ写真のやりとりが残っていれば、Gemini を呼ばずにその応答を返す (`[ocr] reuse_dumps`)。
    if let Some(result) = state.ocr.reuse_dumped(&image_bytes, &mime_type).await {
        return Ok(Json(result));
    }

    // 同じユーザーの読み取りを1件ずつにしてから、枠の残りを見る (`InFlight`)。
    let _in_flight = state.ocr_in_flight.begin(user.id)?;
    ocr_usage::ensure_budget(&state.pool, user.id, state.ocr_free_budget_yen).await?;

    match state.ocr.extract(&image_bytes, &mime_type).await {
        Ok(result) => {
            record_spent(&state, user.id, &result.usage).await;
            Ok(Json(result))
        }
        Err(err) => {
            // 応答が届いて課金された回 (解釈に失敗した・MAX_TOKENS で打ち切られた) も額を足す。
            // 足さないと、毎回その失敗になる写真で累計が進まないまま費用がかかり続ける。
            if let OcrError::ParseResponse { usage: Some(usage) } = &err {
                record_spent(&state, user.id, usage).await;
            }
            Err(err.into())
        }
    }
}

#[utoipa::path(
    post,
    path = "/ocr/consent",
    responses(
        (status = 204, description = "写真を Gemini API に送ることに同意した (同意済みなら何もしない)"),
        (status = 401, description = "未ログイン", body = ErrorResponse),
    )
)]
async fn consent_ocr(
    user: AuthUser,
    State(state): State<AppState>,
) -> Result<StatusCode, AppError> {
    ocr_consent::consent(&state.pool, user.id).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[utoipa::path(
    delete,
    path = "/ocr/consent",
    responses(
        (status = 204, description = "同意を取り消した (同意していなければ何もしない)"),
        (status = 401, description = "未ログイン", body = ErrorResponse),
    )
)]
async fn withdraw_ocr_consent(
    user: AuthUser,
    State(state): State<AppState>,
) -> Result<StatusCode, AppError> {
    ocr_consent::withdraw(&state.pool, user.id).await?;
    Ok(StatusCode::NO_CONTENT)
}

/// 使った額を累計に足す。足せなくても、読み取れた結果 (や失敗の内容) は返す (足せなかった分は無料になるだけ)。
async fn record_spent(state: &AppState, user_id: i64, usage: &OcrUsage) {
    let milli_yen = state
        .ocr_pricing
        .cost_milli_yen(usage.input_tokens, usage.output_tokens);
    if let Err(err) =
        ocr_usage::add_spent(&state.pool, user_id, milli_yen, state.ocr_free_budget_yen).await
    {
        tracing::warn!(error = %crate::error_chain_line(&err), "OCR の使った額を記録できませんでした");
    }
}

/// `multipart/form-data` のボディ読み取り時のエラーを `AppError` に変換する。
/// `axum::extract::DefaultBodyLimit` (このルーターでは `OCR_BODY_LIMIT_BYTES`) を
/// 超えた場合も `MultipartError` としてここに来るが、その場合は `status()` が
/// `413 PAYLOAD_TOO_LARGE` を返すため、汎用の `invalid_request_body` ではなく
/// 意味の通る `ocr_image_too_large` に寄せる (`OCR_MAX_UPLOAD_BYTES` 超過の
/// 明示チェックと同じコードになるようにするため)。
fn multipart_error_to_app_error(err: MultipartError) -> AppError {
    if err.status() == StatusCode::PAYLOAD_TOO_LARGE {
        return AppError::OcrImageTooLarge;
    }
    AppError::InvalidMultipart {
        status: err.status(),
        message: err.body_text(),
    }
}

/// `OcrError::Disabled` はハンドラ冒頭の `state.ocr.enabled()` チェックで弾いているため
/// ここには来ない想定だが、念のため同じ変換をしておく。
impl From<OcrError> for AppError {
    fn from(err: OcrError) -> Self {
        match err {
            OcrError::Disabled => AppError::OcrDisabled,
            OcrError::Request(_) | OcrError::UpstreamStatus(_) | OcrError::ParseResponse { .. } => {
                AppError::OcrUpstream {
                    message: err.to_string(),
                    cause: Some(err),
                }
            }
        }
    }
}

pub fn router() -> OpenApiRouter<AppState> {
    OpenApiRouter::new()
        .routes(routes!(ocr_status))
        .routes(routes!(ocr_extract))
        // 同一パスの POST と DELETE なので1回の routes!() にまとめる。
        .routes(routes!(consent_ocr, withdraw_ocr_consent))
        .layer(DefaultBodyLimit::max(OCR_BODY_LIMIT_BYTES))
}
