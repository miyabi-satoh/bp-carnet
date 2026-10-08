//! 写真を Gemini API に送ることへの同意 (docs/ocr.md)。
//!
//! App Store Review Guidelines 5.1.2(i) が、第三者の AI に個人データを送る前に明示の許可を求めるため。

use sqlx::SqlitePool;

use crate::error::AppError;

pub async fn is_consented(pool: &SqlitePool, user_id: i64) -> Result<bool, AppError> {
    let consented = sqlx::query_scalar!(
        r#"SELECT ocr_consented_at IS NOT NULL AS "consented!: bool" FROM users WHERE id = ?"#,
        user_id
    )
    .fetch_one(pool)
    .await?;
    Ok(consented)
}

/// 同意していなければ `AppError::OcrConsentRequired`。
pub async fn ensure_consented(pool: &SqlitePool, user_id: i64) -> Result<(), AppError> {
    if is_consented(pool, user_id).await? {
        Ok(())
    } else {
        Err(AppError::OcrConsentRequired)
    }
}

/// 同意を記録する。同意済みなら最初の時刻を残す。
pub async fn consent(pool: &SqlitePool, user_id: i64) -> Result<(), AppError> {
    sqlx::query!(
        "UPDATE users SET ocr_consented_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
         WHERE id = ? AND ocr_consented_at IS NULL",
        user_id
    )
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn withdraw(pool: &SqlitePool, user_id: i64) -> Result<(), AppError> {
    sqlx::query!(
        "UPDATE users SET ocr_consented_at = NULL WHERE id = ?",
        user_id
    )
    .execute(pool)
    .await?;
    Ok(())
}
