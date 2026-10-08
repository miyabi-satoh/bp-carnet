//! ログイン中ユーザーの個人設定 (タイムゾーン・朝/夜の時間帯) の取得・更新。

use axum::Json;
use axum::extract::State;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;

use crate::auth::AuthUser;
use crate::error::{AppError, AppJson, ErrorResponse};
use crate::settings::{self, PeriodSettings};
use crate::state::AppState;
use crate::timezones::TIMEZONE_CHOICES;
use crate::validation::{
    self, PeriodThresholds, PeriodThresholdsError, is_known_timezone, validate_period_thresholds,
};

/// 個人設定。時刻は「その日の 0:00 からの経過分数」で表す (24:00 を表せるよう上限は 1440)。
#[derive(Debug, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SettingsResponse {
    /// IANA タイムゾーン名。自動設定のときは、最後にアプリを開いたブラウザのタイムゾーン。
    #[schema(example = "Asia/Tokyo")]
    timezone: String,
    /// タイムゾーンが自動設定か。
    timezone_auto: bool,
    #[schema(example = 240)]
    morning_start_min: i64,
    #[schema(example = 600)]
    morning_end_min: i64,
    #[schema(example = 1080)]
    evening_start_min: i64,
    #[schema(example = 1440)]
    evening_end_min: i64,
}

impl From<PeriodSettings> for SettingsResponse {
    fn from(value: PeriodSettings) -> Self {
        Self {
            timezone: value.timezone,
            timezone_auto: value.timezone_auto,
            morning_start_min: value.thresholds.morning_start_min,
            morning_end_min: value.thresholds.morning_end_min,
            evening_start_min: value.thresholds.evening_start_min,
            evening_end_min: value.thresholds.evening_end_min,
        }
    }
}

/// 更新リクエスト。全項目を必須にして、部分更新は扱わない (設定画面が常に全項目を持つため)。
#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UpdateSettingsRequest {
    /// 自動設定にするときは、ブラウザのタイムゾーンを送る。
    #[schema(example = "Asia/Tokyo")]
    timezone: String,
    /// 自動設定にするか (手動で選んだら `false`)。
    timezone_auto: bool,
    #[schema(example = 240)]
    morning_start_min: i64,
    #[schema(example = 600)]
    morning_end_min: i64,
    #[schema(example = 1080)]
    evening_start_min: i64,
    #[schema(example = 1440)]
    evening_end_min: i64,
}

/// `PeriodThresholdsError` を API のエラーに変換する。
///
/// 表示用の文言はフロントエンド側が持つ (入力欄の隣にどの規則に反したかを出すため、
/// 送信前に同じ規則で検証している)。ここはあくまで最後の砦なので、`Validation` に
/// 集約して開発者向けの説明だけを載せる。
fn threshold_error(error: PeriodThresholdsError) -> AppError {
    let detail = match error {
        PeriodThresholdsError::OutOfRange => {
            let range = validation::DAY_MINUTES_RANGE;
            format!(
                "period thresholds must be within {}..={}",
                range.start(),
                range.end()
            )
        }
        PeriodThresholdsError::NotAscending => "each period must start before it ends".to_string(),
        PeriodThresholdsError::Overlapping => {
            "morning and evening periods must not overlap".to_string()
        }
    };
    AppError::Validation(detail)
}

/// 設定画面のタイムゾーン選択肢。
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct TimezonesResponse {
    /// 手動で選べる IANA タイムゾーン名 (`timezones::TIMEZONE_CHOICES`)。並びは表示側が決める。
    timezones: Vec<String>,
}

/// 選択肢をサーバーから配る。一覧のうち、サーバー同梱の tzdb にある名前だけを返す (tzdb の版しだいで
/// 「選べるのに保存できない」値を出さないため)。
#[utoipa::path(
    get,
    path = "/settings/timezones",
    responses(
        (status = 200, description = "取得成功", body = TimezonesResponse),
        (status = 401, description = "未ログイン", body = ErrorResponse),
    )
)]
async fn list_timezones(_user: AuthUser) -> Json<TimezonesResponse> {
    Json(TimezonesResponse {
        timezones: TIMEZONE_CHOICES
            .iter()
            .filter(|zone| is_known_timezone(zone))
            .map(|zone| zone.to_string())
            .collect(),
    })
}

#[utoipa::path(
    get,
    path = "/settings",
    responses(
        (status = 200, description = "取得成功", body = SettingsResponse),
        (status = 401, description = "未ログイン", body = ErrorResponse),
    )
)]
async fn get_settings(
    user: AuthUser,
    State(state): State<AppState>,
) -> Result<Json<SettingsResponse>, AppError> {
    let settings = settings::load_required(&state.pool, user.id).await?;
    Ok(Json(settings.into()))
}

#[utoipa::path(
    put,
    path = "/settings",
    request_body = UpdateSettingsRequest,
    responses(
        (status = 200, description = "更新成功", body = SettingsResponse),
        (status = 400, description = "リクエストボディが不正 (JSON として解釈できない等)", body = ErrorResponse),
        (status = 401, description = "未ログイン", body = ErrorResponse),
        (status = 415, description = "Content-Type が application/json でない", body = ErrorResponse),
        (status = 422, description = "タイムゾーン名が tzdb に無い、または朝/夜の時間帯が値域外・開始>=終了・重複", body = ErrorResponse),
    )
)]
async fn update_settings(
    user: AuthUser,
    State(state): State<AppState>,
    AppJson(payload): AppJson<UpdateSettingsRequest>,
) -> Result<Json<SettingsResponse>, AppError> {
    check_timezone(&payload.timezone)?;

    let thresholds = PeriodThresholds {
        morning_start_min: payload.morning_start_min,
        morning_end_min: payload.morning_end_min,
        evening_start_min: payload.evening_start_min,
        evening_end_min: payload.evening_end_min,
    };
    validate_period_thresholds(thresholds).map_err(threshold_error)?;

    let settings = PeriodSettings {
        timezone: payload.timezone,
        timezone_auto: payload.timezone_auto,
        thresholds,
    };
    settings::save(&state.pool, user.id, &settings).await?;

    Ok(Json(settings.into()))
}

/// tzdb に無いタイムゾーン名を拒否する。自動設定ではブラウザの値が来るため、選択肢の一覧ではなく tzdb 全件で見る。
fn check_timezone(timezone: &str) -> Result<(), AppError> {
    if is_known_timezone(timezone) {
        Ok(())
    } else {
        Err(AppError::Validation(format!(
            "unknown timezone: {timezone}"
        )))
    }
}

/// ブラウザで決まったタイムゾーン。
#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DetectedTimezoneRequest {
    /// `Intl.DateTimeFormat().resolvedOptions().timeZone` の値。
    #[schema(example = "Asia/Tokyo")]
    timezone: String,
}

/// 自動設定のユーザーのタイムゾーンを、アプリを開いたブラウザのタイムゾーンに揃える。手動設定のユーザーは
/// 変えずに今の設定を返す (別の端末で手動にした直後に上書きしないため)。
#[utoipa::path(
    put,
    path = "/settings/detected-timezone",
    request_body = DetectedTimezoneRequest,
    responses(
        (status = 200, description = "更新後 (手動設定なら変えないまま) の設定", body = SettingsResponse),
        (status = 400, description = "リクエストボディが不正 (JSON として解釈できない等)", body = ErrorResponse),
        (status = 401, description = "未ログイン", body = ErrorResponse),
        (status = 415, description = "Content-Type が application/json でない", body = ErrorResponse),
        (status = 422, description = "タイムゾーン名が tzdb に無い", body = ErrorResponse),
    )
)]
async fn update_detected_timezone(
    user: AuthUser,
    State(state): State<AppState>,
    AppJson(payload): AppJson<DetectedTimezoneRequest>,
) -> Result<Json<SettingsResponse>, AppError> {
    check_timezone(&payload.timezone)?;
    settings::save_detected_timezone(&state.pool, user.id, &payload.timezone).await?;
    let settings = settings::load_required(&state.pool, user.id).await?;
    Ok(Json(settings.into()))
}

pub fn router() -> OpenApiRouter<AppState> {
    // 同一パス "/settings" の GET と PUT なので1回の routes!() にまとめる。
    // "/settings/timezones"・"/settings/detected-timezone" は別パスなので分ける。
    OpenApiRouter::new()
        .routes(routes!(get_settings, update_settings))
        .routes(routes!(list_timezones))
        .routes(routes!(update_detected_timezone))
}
