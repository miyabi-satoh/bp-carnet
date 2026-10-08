//! ユーザーごとの個人設定 (タイムゾーン・朝/夜の時間帯) の読み書き。
//!
//! 朝/夜の集計 (`stats`) と設定 API の両方が同じ行を読むため、クエリをここに集約する。

use sqlx::SqlitePool;

use crate::error::AppError;
use crate::local_time::UserTimeZone;
use crate::stats::SlotClassifier;
use crate::validation::PeriodThresholds;

/// `users` に保存されている個人設定。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PeriodSettings {
    /// IANA タイムゾーン名。自動設定のときも具体的な名前を持ち、サーバーの計算はこの値で行う。
    pub timezone: String,
    /// 自動設定か。`true` なら、アプリを開いたブラウザのタイムゾーンで `timezone` を更新する。
    pub timezone_auto: bool,
    pub thresholds: PeriodThresholds,
}

impl PeriodSettings {
    /// 保存されているタイムゾーン名を解決する。tzdb に無い名前 (DB の直接編集や tzdb からの
    /// ゾーン廃止で起きうる) は、日時の変換も表示もできないためエラーにする。
    pub fn user_timezone(&self) -> Result<UserTimeZone, AppError> {
        UserTimeZone::get(&self.timezone)
            .ok_or_else(|| AppError::UnknownTimezone(self.timezone.clone()))
    }
}

/// 指定ユーザーの個人設定を読む。ユーザーが存在しなければ `None`。
pub async fn load(pool: &SqlitePool, user_id: i64) -> Result<Option<PeriodSettings>, sqlx::Error> {
    let row = sqlx::query!(
        r#"
        SELECT
            timezone AS "timezone!: String",
            timezone_auto AS "timezone_auto!: bool",
            morning_start_min AS "morning_start_min!: i64",
            morning_end_min AS "morning_end_min!: i64",
            evening_start_min AS "evening_start_min!: i64",
            evening_end_min AS "evening_end_min!: i64"
        FROM users
        WHERE id = ?
        "#,
        user_id,
    )
    .fetch_optional(pool)
    .await?;

    Ok(row.map(|row| PeriodSettings {
        timezone: row.timezone,
        timezone_auto: row.timezone_auto,
        thresholds: PeriodThresholds {
            morning_start_min: row.morning_start_min,
            morning_end_min: row.morning_end_min,
            evening_start_min: row.evening_start_min,
            evening_end_min: row.evening_end_min,
        },
    }))
}

/// 指定ユーザーの個人設定を読む。ユーザーが存在しなければ `AppError::NotFound`。
pub async fn load_required(pool: &SqlitePool, user_id: i64) -> Result<PeriodSettings, AppError> {
    load(pool, user_id).await?.ok_or(AppError::NotFound)
}

/// 指定ユーザーのタイムゾーンを読む。ユーザーが存在しなければ `AppError::NotFound`。
pub async fn load_timezone(pool: &SqlitePool, user_id: i64) -> Result<UserTimeZone, AppError> {
    load_required(pool, user_id).await?.user_timezone()
}

/// 指定ユーザーの個人設定から朝/夜の判定器 (タイムゾーンを含む) を作る。記録の一覧・集計の両方に渡す。
pub async fn load_slot_classifier(
    pool: &SqlitePool,
    user_id: i64,
) -> Result<SlotClassifier, AppError> {
    let settings = load_required(pool, user_id).await?;
    Ok(SlotClassifier::new(
        settings.user_timezone()?,
        settings.thresholds,
    ))
}

/// 指定ユーザーの個人設定を上書きする。値の妥当性は呼び出し側 (`validation`) で検証済みの前提。
pub async fn save(
    pool: &SqlitePool,
    user_id: i64,
    settings: &PeriodSettings,
) -> Result<(), sqlx::Error> {
    let PeriodThresholds {
        morning_start_min,
        morning_end_min,
        evening_start_min,
        evening_end_min,
    } = settings.thresholds;

    sqlx::query!(
        r#"
        UPDATE users
        SET timezone = ?,
            timezone_auto = ?,
            morning_start_min = ?,
            morning_end_min = ?,
            evening_start_min = ?,
            evening_end_min = ?
        WHERE id = ?
        "#,
        settings.timezone,
        settings.timezone_auto,
        morning_start_min,
        morning_end_min,
        evening_start_min,
        evening_end_min,
        user_id,
    )
    .execute(pool)
    .await?;

    Ok(())
}

/// 自動設定のユーザーだけ、ブラウザで決まったタイムゾーンで `timezone` を更新する。名前の妥当性は
/// 呼び出し側で検証済みの前提。手動設定のユーザーは変えない (別の端末で手動にした直後に、
/// 自動のままの端末が上書きしないため)。
pub async fn save_detected_timezone(
    pool: &SqlitePool,
    user_id: i64,
    timezone: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query!(
        "UPDATE users SET timezone = ? WHERE id = ? AND timezone_auto = 1",
        timezone,
        user_id,
    )
    .execute(pool)
    .await?;
    Ok(())
}
