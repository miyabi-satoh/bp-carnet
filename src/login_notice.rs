//! 新しい端末からのログイン (アプリは毎回) を、本人にメールで知らせる (docs/authentication.md「ログインの記録と知らせ」)。
//!
//! 第三者に入られたときに本人が気づき、パスワードの再設定で取り戻せるようにするため。

use jiff::Timestamp;
use sqlx::SqlitePool;

use crate::login_events::{Client, Method};
use crate::mail::Mailer;
use crate::state::AppState;

const SUBJECT: &str = "【BP Carnet】新しいログインがありました";

/// 応答を待たずに [`send`] する。SMTP の往復でログインを待たせないため。ログインが成立した直後に呼ぶ
/// (呼んだ時刻をログインの日時として書く)。
///
/// 同じアカウントへの知らせは `login_notice_rate_limiter` の枠まで (10分に5回)。正しいパスワードでのログインは
/// 試行の枠を返すので、ここで数えないと、入れる人がメールを際限なく送らせられる。送れなくてもログインは取り消さない。
pub fn spawn(state: &AppState, user_id: i64, method: Method, client: Client) {
    if state
        .login_notice_rate_limiter
        .try_acquire(&user_id.to_string())
        .is_none()
    {
        tracing::info!(user_id, "ログインのお知らせの上限に達したため、送りません");
        return;
    }
    let state = state.clone();
    let logged_in_at = Timestamp::now();
    tokio::spawn(async move {
        if let Err(err) = send(
            &state.pool,
            &state.mailer,
            user_id,
            method,
            client,
            logged_in_at,
        )
        .await
        {
            tracing::error!(error = %crate::error_chain_line(&err), user_id, "ログインのお知らせを送れませんでした");
        }
    });
}

/// ユーザー ID がメールアドレスでないときは何もしない (`crate::password_notice::send` と同じ)。
async fn send(
    pool: &SqlitePool,
    mailer: &Mailer,
    user_id: i64,
    method: Method,
    client: Client,
    logged_in_at: Timestamp,
) -> Result<(), crate::password_notice::Error> {
    let Some(username) = crate::auth::load_username(pool, user_id).await? else {
        return Ok(());
    };
    if !mailer.can_send_to(&username) {
        return Ok(());
    }
    let logged_in_at = crate::password_notice::local_time_label(pool, user_id, logged_in_at).await;
    let reset_link = mailer.link("/reset-password");
    mailer
        .send(
            &username,
            SUBJECT,
            body(&logged_in_at, method, client, &reset_link),
        )
        .await?;
    Ok(())
}

fn body(logged_in_at: &str, method: Method, client: Client, reset_link: &str) -> String {
    // 外部のログインなら、乗っ取られたのはその提供元のアカウントかもしれない。
    let what_to_do = match method {
        Method::Google | Method::Line | Method::Apple => format!(
            "心当たりが無い場合は、{} のアカウントのパスワードを変え、次のリンクから BP Carnet のパスワードも再設定してください。",
            method.label()
        ),
        Method::Password | Method::Signup => {
            "心当たりが無い場合は、次のリンクからパスワードを再設定してください。".to_string()
        }
    };
    format!(
        "BP Carnet のアカウントにログインがありました。\n\
         \n\
         日時: {logged_in_at}\n\
         方法: {}（{}）\n\
         \n\
         {what_to_do}再設定すると、ほかの端末のログインもすべて解除されます。\n\
         \n\
         {reset_link}\n\
         \n\
         心当たりがある場合は、このメールは破棄してください。\n",
        method.label(),
        client.label()
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::insert_user;

    #[sqlx::test]
    async fn send_writes_the_method_and_the_client(pool: SqlitePool) {
        let user_id = insert_user(&pool, "alice@example.com").await;
        let (mailer, sent) = Mailer::recording("https://bp.example.com");

        send(
            &pool,
            &mailer,
            user_id,
            Method::Google,
            Client::App,
            Timestamp::UNIX_EPOCH,
        )
        .await
        .expect("送れるはず");

        let sent = sent.lock().expect("lock");
        assert_eq!(sent.len(), 1);
        assert_eq!(sent[0].to, "alice@example.com");
        assert_eq!(sent[0].subject, SUBJECT);
        assert!(
            sent[0].body.contains("方法: Google（アプリ）"),
            "{}",
            sent[0].body
        );
        assert!(
            sent[0]
                .body
                .contains("Google のアカウントのパスワードを変え、次のリンクから BP Carnet のパスワードも再設定してください"),
            "{}",
            sent[0].body
        );
        // 既定のタイムゾーン (Asia/Tokyo) で書く。
        assert!(
            sent[0].body.contains("日時: 1970年1月1日 09:00"),
            "{}",
            sent[0].body
        );
        assert!(
            sent[0]
                .body
                .contains("https://bp.example.com/reset-password"),
            "{}",
            sent[0].body
        );
    }

    #[sqlx::test]
    async fn send_skips_a_user_id_that_is_not_an_email_address(pool: SqlitePool) {
        let user_id = insert_user(&pool, "alice").await;
        let (mailer, sent) = Mailer::recording("https://bp.example.com");

        send(
            &pool,
            &mailer,
            user_id,
            Method::Password,
            Client::Web,
            Timestamp::UNIX_EPOCH,
        )
        .await
        .expect("何もせずに済むはず");

        assert!(sent.lock().expect("lock").is_empty());
    }
}
