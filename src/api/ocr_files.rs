//! 他のアプリが書き出したファイルの読み取り (Gemini 連携、docs/import-export.md「他のアプリのファイル」)。

use axum::Json;
use axum::extract::{DefaultBodyLimit, State};
use serde::Deserialize;
use utoipa::ToSchema;
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;

use super::ocr::{read_upload_field, spend};
use crate::auth::AuthUser;
use crate::error::{AppError, AppJson, AppMultipart, ErrorResponse};
use crate::ocr::OcrError;
use crate::ocr_files::{DocumentReadings, TABLE_SAMPLE_MAX_CHARS, TableColumns};
use crate::ocr_usage;
use crate::state::AppState;

/// 送れる PDF の上限。Gemini に本文へ埋めて送れる大きさ (リクエスト全体で 20MB、base64 で約1.33倍になる) に
/// 収まり、アプリが書き出す血圧の一覧 (数百KB〜数MB) には十分な大きさ。
const DOCUMENT_MAX_UPLOAD_BYTES: usize = 10 * 1024 * 1024;

/// axum の `DefaultBodyLimit` に渡す上限。`multipart/form-data` のオーバーヘッドの分だけ余裕を持たせ、
/// 実際の上限の判定はハンドラの中でする (`api::ocr` の `OCR_BODY_LIMIT_BYTES` と同じ)。
const DOCUMENT_BODY_LIMIT_BYTES: usize = DOCUMENT_MAX_UPLOAD_BYTES + 64 * 1024;

/// PDF の先頭のバイト列。拡張子や `Content-Type` は送る側が決められるので、中身で確かめる。
const PDF_MAGIC: &[u8] = b"%PDF-";

#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct TableColumnsRequest {
    /// ファイルの先頭の数行 (見出しと、列を決めるのに足りる数行のデータ)。8,000 文字まで。
    sample: String,
}

/// OpenAPI ドキュメント用のダミー型 (実際の抽出は `AppMultipart` で行う。`api::ocr` の `OcrUploadForm` と同じ)。
#[derive(ToSchema)]
#[allow(dead_code)]
struct DocumentUploadForm {
    #[schema(value_type = String, format = Binary)]
    file: Vec<u8>,
}

#[utoipa::path(
    post,
    path = "/ocr/table-columns",
    request_body = TableColumnsRequest,
    responses(
        (status = 200, description = "列の意味 (血圧の記録の表でなければ `readable: false`)", body = TableColumns),
        (status = 400, description = "リクエストボディが不正 (JSON として解釈できない等)", body = ErrorResponse),
        (status = 401, description = "未ログイン", body = ErrorResponse),
        (status = 409, description = "同じユーザーの読み取りがまだ終わっていない (`ocr_in_progress`)", body = ErrorResponse),
        (status = 415, description = "Content-Type が application/json でない", body = ErrorResponse),
        (status = 422, description = "`sample` が空、または 8,000 文字を超える", body = ErrorResponse),
        (status = 429, description = "全部の読み取りの枠を使い切った (`ocr_budget_exhausted`)", body = ErrorResponse),
        (status = 502, description = "Gemini の呼び出し、またはレスポンスの解釈に失敗", body = ErrorResponse),
        (status = 503, description = "GEMINI_API_KEY 未設定でOCR機能が無効", body = ErrorResponse),
    )
)]
async fn table_columns(
    user: AuthUser,
    State(state): State<AppState>,
    AppJson(request): AppJson<TableColumnsRequest>,
) -> Result<Json<TableColumns>, AppError> {
    if !state.ocr.enabled() {
        return Err(AppError::OcrDisabled);
    }
    if request.sample.trim().is_empty() {
        return Err(AppError::Validation("sample must not be empty".to_string()));
    }
    if request.sample.chars().count() > TABLE_SAMPLE_MAX_CHARS {
        return Err(AppError::Validation(format!(
            "sample must be at most {TABLE_SAMPLE_MAX_CHARS} characters"
        )));
    }

    // 同じユーザーの読み取りを1件ずつにしてから、枠の残りを見る (`InFlight`、写真と共通)。
    let _in_flight = state.ocr_in_flight.begin(user.id)?;
    ocr_usage::ensure_budget(&state.pool, user.id, state.ocr_free_budget_yen).await?;

    let result = state.ocr.table_columns(&request.sample).await;
    Ok(Json(spend(&state, user.id, result).await?))
}

#[utoipa::path(
    post,
    path = "/ocr/document",
    request_body(content = DocumentUploadForm, content_type = "multipart/form-data"),
    responses(
        (status = 200, description = "読み取った記録 (血圧の記録が無い文書なら空)", body = DocumentReadings),
        (status = 400, description = "multipart ボディが不正、または file フィールドが無い", body = ErrorResponse),
        (status = 401, description = "未ログイン", body = ErrorResponse),
        (status = 409, description = "同じユーザーの読み取りがまだ終わっていない (`ocr_in_progress`)", body = ErrorResponse),
        (status = 413, description = "ファイルが10MBを超える (`ocr_file_too_large`)", body = ErrorResponse),
        (status = 422, description = "PDF でない (`ocr_unsupported_file`)、または記録が多すぎて読み取りが途中で切れた (`ocr_document_too_long`)", body = ErrorResponse),
        (status = 429, description = "全部の読み取りの枠を使い切った (`ocr_budget_exhausted`)", body = ErrorResponse),
        (status = 502, description = "Gemini の呼び出し、またはレスポンスの解釈に失敗", body = ErrorResponse),
        (status = 503, description = "GEMINI_API_KEY 未設定でOCR機能が無効", body = ErrorResponse),
    )
)]
async fn read_document(
    user: AuthUser,
    State(state): State<AppState>,
    AppMultipart(mut multipart): AppMultipart,
) -> Result<Json<DocumentReadings>, AppError> {
    if !state.ocr.enabled() {
        return Err(AppError::OcrDisabled);
    }

    let (_, file) = read_upload_field(&mut multipart, "file", || AppError::OcrFileTooLarge).await?;
    if file.len() > DOCUMENT_MAX_UPLOAD_BYTES {
        return Err(AppError::OcrFileTooLarge);
    }
    if !file.starts_with(PDF_MAGIC) {
        return Err(AppError::OcrUnsupportedFile);
    }

    let _in_flight = state.ocr_in_flight.begin(user.id)?;
    ocr_usage::ensure_budget(&state.pool, user.id, state.ocr_free_budget_yen).await?;

    let result = state.ocr.read_document(&file).await;
    spend(&state, user.id, result)
        .await
        .map(Json)
        .map_err(|err| match err {
            // 記録の数を減らせば読めるので、Gemini の失敗 (502) と分けて伝える。
            OcrError::Truncated { .. } => AppError::OcrDocumentTooLong,
            err => err.into(),
        })
}

pub fn router() -> OpenApiRouter<AppState> {
    OpenApiRouter::new()
        .routes(routes!(read_document))
        .layer(DefaultBodyLimit::max(DOCUMENT_BODY_LIMIT_BYTES))
        // 上の上限は PDF を受ける口だけに付ける。JSON の口は、ほかの API と同じ既定の上限のままにする。
        .routes(routes!(table_columns))
}
