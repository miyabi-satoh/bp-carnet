//! 本人へのお知らせのメール (パスワードの変更 `crate::password_notice`・新しいログイン `crate::login_notice`) で共通の部品。

use jiff::Timestamp;
use sqlx::SqlitePool;

use crate::mail::Mailer;
use crate::settings;

/// お知らせを送る先 (ユーザー ID のメールアドレス)。ユーザー ID がメールアドレスでなく送れないときは `None`。
pub(crate) async fn recipient(
    pool: &SqlitePool,
    mailer: &Mailer,
    user_id: i64,
) -> Result<Option<String>, sqlx::Error> {
    let username = crate::auth::load_username(pool, user_id).await?;
    Ok(username.filter(|username| mailer.can_send_to(username)))
}

const NOTICE_TIME_FORMAT: &str = "%Y年%-m月%-d日 %H:%M";

/// お知らせの本文に書く日時 (本人のタイムゾーン)。
///
/// ADR: 本人のタイムゾーンを読めなくても、UTC と断って書いて送る。勝手に変えられた・入られたことに
/// 気づいてもらうのが目的なので、日時の書き方のためにお知らせを諦めない。
pub(crate) async fn local_time_label(pool: &SqlitePool, user_id: i64, at: Timestamp) -> String {
    match settings::load_timezone(pool, user_id).await {
        Ok(timezone) => timezone
            .to_local(at)
            .strftime(NOTICE_TIME_FORMAT)
            .to_string(),
        Err(err) => {
            tracing::warn!(error = %crate::error_chain_line(&err), user_id, "タイムゾーンを読めないため、お知らせの日時を UTC で書きます");
            format!("{} (UTC)", at.strftime(NOTICE_TIME_FORMAT))
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("ユーザーを読めません")]
    Db(#[from] sqlx::Error),
    #[error(transparent)]
    Mail(#[from] crate::mail::Error),
}
