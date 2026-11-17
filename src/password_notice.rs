//! パスワードが変わったことを本人にメールで知らせる (docs/authentication.md)。
//!
//! 勝手に変えられたときに本人が気づき、メールでの再設定で取り戻せるようにするため。

use jiff::Timestamp;
use sqlx::SqlitePool;

use crate::mail::Mailer;
use crate::notice;
use crate::state::AppState;

/// パスワードが変わった経路。本文で、どうやって変わったかを伝える。
#[derive(Debug, Clone, Copy)]
pub enum Origin {
    /// 本人が設定画面から変えた。
    User,
    /// メールのリンクから再設定した。
    EmailLink,
    /// 管理者が管理画面から再設定した。
    Admin,
    /// サーバーのマシンの端末から再設定した (`--reset-password`)。
    Terminal,
}

impl Origin {
    fn label(self) -> &'static str {
        match self {
            Self::User => "設定画面からの変更",
            Self::EmailLink => "メールのリンクからの再設定",
            Self::Admin => "管理者による再設定",
            Self::Terminal => "サーバーの管理者による再設定",
        }
    }
}

const SUBJECT: &str = "【BP Carnet】パスワードが変更されました";

/// API の経路から、応答を待たずに [`send`] する。SMTP の往復で変更の操作を待たせないため。
/// 変更が成立した直後に呼ぶこと (呼んだ時刻を変更の日時として書く)。
/// 送れなくても変更は成立しているので、失敗はログに残すだけにする。
pub fn spawn(state: &AppState, user_id: i64, origin: Origin) {
    let state = state.clone();
    let changed_at = Timestamp::now();
    tokio::spawn(async move {
        if let Err(err) = send(&state.pool, &state.mailer, user_id, origin, changed_at).await {
            tracing::error!(error = %crate::error_chain_line(&err), user_id, "パスワード変更のお知らせを送れませんでした");
        }
    });
}

/// `user_id` のパスワードが `changed_at` に変わったことを知らせる。ユーザー ID がメールアドレスでない
/// ときは何もしない。
pub async fn send(
    pool: &SqlitePool,
    mailer: &Mailer,
    user_id: i64,
    origin: Origin,
    changed_at: Timestamp,
) -> Result<(), notice::Error> {
    let Some(username) = notice::recipient(pool, mailer, user_id).await? else {
        return Ok(());
    };
    let changed_at = notice::local_time_label(pool, user_id, changed_at).await;
    let reset_link = mailer.link("/reset-password");
    mailer
        .send(&username, SUBJECT, body(&changed_at, origin, &reset_link))
        .await?;
    Ok(())
}

fn body(changed_at: &str, origin: Origin, reset_link: &str) -> String {
    format!(
        "BP Carnet のアカウントのパスワードが変更されました。\n\
         \n\
         日時: {changed_at}\n\
         方法: {}\n\
         \n\
         変更した覚えが無い場合は、第三者にパスワードを変えられたおそれがあります。\n\
         次のリンクからパスワードを再設定してください。再設定すると、ほかの端末のログインもすべて解除されます。\n\
         \n\
         {reset_link}\n\
         \n\
         心当たりがある場合は、このメールは破棄してください。\n",
        origin.label()
    )
}
