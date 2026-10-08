//! 放置アカウントの自動退会 (docs/authentication.md)。
//!
//! 最終アクセスから `[inactivity] delete_after_days` 日がたつ前に、30日前・7日前・前日の予告を
//! メールで送る。過ぎたら通常の削除予約 ([`crate::account`]) を入れ、物理削除はそちらの定期処理に任せる。

use jiff::{SignedDuration, Timestamp};
use sqlx::SqlitePool;

use crate::account::{self, DeletionOrigin};
use crate::mail::Mailer;

/// 予告を送る、退会日までの残り日数。大きいほうから順に送る。
const NOTICE_DAYS: [i64; 3] = [30, 7, 1];

/// 退会日までの残りに対して、いま送るべき予告 (残り日数)。まだ最初の予告の時期でなければ `None`。
///
/// 定期処理が止まっていて時期を過ぎたときも、該当する一番近い予告を1通だけ返す。
fn due_notice(remaining: SignedDuration, delete_after_days: u32) -> Option<i64> {
    let days = (remaining.as_secs() + 24 * 60 * 60 - 1).div_euclid(24 * 60 * 60);
    // 設定の期間より長い前の予告 (期間が7日なのに「30日前」) は存在しないので送らない。
    NOTICE_DAYS
        .iter()
        .rev()
        .copied()
        .filter(|&n| n < i64::from(delete_after_days))
        .find(|&n| days <= n)
}

/// 対象のユーザーを1回ずつ見て、予告を送るか、削除予約を入れる。`delete_after_days` が 0 なら何もしない。
/// 1人の失敗で全体を止めず、ログに残して次の周期に回す。
pub async fn run_once(
    pool: &SqlitePool,
    mailer: &Mailer,
    delete_after_days: u32,
) -> Result<(), sqlx::Error> {
    if delete_after_days == 0 {
        return Ok(());
    }
    let now = Timestamp::now();
    let rows = sqlx::query!(
        r#"SELECT id AS "id!: i64", username,
                  COALESCE(last_seen_at, created_at) AS "last_seen!: String",
                  inactivity_notice_days
           FROM users
           WHERE role = 'user' AND frozen = 0 AND email_verified = 1
                 AND deletion_scheduled_at IS NULL
                 -- 買い足した枠が残っていれば、払って得たものを消さないよう対象にしない。
                 AND NOT EXISTS (SELECT 1 FROM ocr_quota_grants
                                 WHERE ocr_quota_grants.user_id = users.id
                                   AND ocr_quota_grants.remaining_milli_yen > 0)"#
    )
    .fetch_all(pool)
    .await?;

    for row in rows {
        let Ok(last_seen) = row.last_seen.parse::<Timestamp>() else {
            tracing::error!(user_id = row.id, "最終アクセスを解釈できませんでした");
            continue;
        };
        let deadline = last_seen + SignedDuration::from_hours(24 * i64::from(delete_after_days));
        let remaining = deadline.duration_since(now);
        let sendable = mailer.can_send_to(&row.username);

        if remaining <= SignedDuration::ZERO {
            if let Err(err) = schedule(pool, mailer, row.id, &row.username, sendable).await {
                tracing::error!(error = %crate::error_chain_line(&err), user_id = row.id, "自動退会の削除予約に失敗しました");
            }
            continue;
        }
        let Some(notice) = due_notice(remaining, delete_after_days) else {
            continue;
        };
        // 送った予告より小さい (近い) 予告だけを送る。同じ予告を重ねて送らない。
        if !sendable
            || row
                .inactivity_notice_days
                .is_some_and(|sent| notice >= sent)
        {
            continue;
        }
        if let Err(err) = send_notice(pool, mailer, row.id, &row.username, &deadline).await {
            tracing::error!(error = %crate::error_chain_line(&err), user_id = row.id, "自動退会の予告メールを送れませんでした");
            continue;
        }
        // 送れたあとの記録に失敗すると同じ予告を送り直すが、送らないよりよい。
        if let Err(err) = sqlx::query!(
            "UPDATE users SET inactivity_notice_days = ? WHERE id = ?",
            notice,
            row.id
        )
        .execute(pool)
        .await
        {
            tracing::error!(error = %crate::error_chain_line(&err), user_id = row.id, "自動退会の予告を記録できませんでした");
        }
    }
    Ok(())
}

/// 退会日を過ぎたユーザーに削除予約を入れ、送れるなら知らせる。
async fn schedule(
    pool: &SqlitePool,
    mailer: &Mailer,
    user_id: i64,
    username: &str,
    sendable: bool,
) -> Result<(), MailOrDb> {
    let mut tx = pool.begin().await?;
    let scheduled = account::schedule_deletion(&mut tx, user_id, DeletionOrigin::Inactive).await?;
    tx.commit().await?;
    let Some(scheduled) = scheduled.filter(|s| s.newly_scheduled) else {
        return Ok(());
    };
    tracing::info!(user_id, "使われていないアカウントの削除を予約しました");
    if sendable {
        let scheduled_at: Timestamp = scheduled.scheduled_at.parse().map_err(|_| MailOrDb::Time)?;
        let date = account::local_date(pool, user_id, scheduled_at)
            .await
            .map_err(|_| MailOrDb::Time)?;
        let login_link = mailer.link("/login");
        mailer
            .send(
                username,
                SCHEDULED_SUBJECT,
                scheduled_body(&date, &login_link),
            )
            .await?;
    }
    Ok(())
}

async fn send_notice(
    pool: &SqlitePool,
    mailer: &Mailer,
    user_id: i64,
    username: &str,
    deadline: &Timestamp,
) -> Result<(), MailOrDb> {
    let date = account::local_date(pool, user_id, *deadline)
        .await
        .map_err(|_| MailOrDb::Time)?;
    let login_link = mailer.link("/login");
    mailer
        .send(username, NOTICE_SUBJECT, notice_body(&date, &login_link))
        .await?;
    Ok(())
}

#[derive(Debug, thiserror::Error)]
enum MailOrDb {
    #[error(transparent)]
    Db(#[from] sqlx::Error),
    #[error(transparent)]
    Mail(#[from] crate::mail::Error),
    #[error("日時を求められませんでした")]
    Time,
}

const NOTICE_SUBJECT: &str = "【BP Carnet】アカウントの自動削除のお知らせ";

fn notice_body(date: &str, login_link: &str) -> String {
    format!(
        "BP Carnet を長い間ご利用いただいていないため、{date}以降にアカウントを自動的に\n\
         削除します。記録データも一緒に消えます。\n\
         \n\
         続けて使う場合は、それまでにログインしてください。\n\
         \n\
         {login_link}\n\
         \n\
         記録を残したい場合は、ログイン後に CSV で書き出せます。\n"
    )
}

const SCHEDULED_SUBJECT: &str = "【BP Carnet】アカウントの削除を予約しました";

fn scheduled_body(date: &str, login_link: &str) -> String {
    format!(
        "BP Carnet を長い間ご利用いただいていないため、アカウントの削除を予約しました。\n\
         {date}以降に、記録データを含めてアカウントを完全に削除します。\n\
         \n\
         それまでにログインすると、削除は自動的に取り消されます。\n\
         \n\
         {login_link}\n"
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::insert_user;

    const DELETE_AFTER: u32 = 365;

    fn days(n: i64) -> SignedDuration {
        SignedDuration::from_hours(24 * n)
    }

    /// 最終アクセスを `ago` 日前にする。
    async fn set_last_seen(pool: &SqlitePool, user_id: i64, ago: i64) {
        sqlx::query!(
            "UPDATE users SET last_seen_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now', printf('-%d days', ?))
             WHERE id = ?",
            ago,
            user_id
        )
        .execute(pool)
        .await
        .expect("最終アクセスを書き換えられるはず");
    }

    async fn state(
        pool: &SqlitePool,
        user_id: i64,
    ) -> (Option<String>, Option<DeletionOrigin>, Option<i64>) {
        let row = sqlx::query!(
            r#"SELECT deletion_scheduled_at, deletion_origin AS "deletion_origin: DeletionOrigin",
                      inactivity_notice_days
               FROM users WHERE id = ?"#,
            user_id
        )
        .fetch_one(pool)
        .await
        .expect("状態を読めるはず");
        (
            row.deletion_scheduled_at,
            row.deletion_origin,
            row.inactivity_notice_days,
        )
    }

    #[test]
    fn notice_is_due_from_30_days_before() {
        assert_eq!(due_notice(days(31), 365), None);
        assert_eq!(due_notice(days(30), 365), Some(30));
        assert_eq!(due_notice(days(8), 365), Some(30));
        assert_eq!(due_notice(days(7), 365), Some(7));
        assert_eq!(due_notice(days(2), 365), Some(7));
        assert_eq!(due_notice(days(1), 365), Some(1));
        assert_eq!(due_notice(SignedDuration::from_hours(3), 365), Some(1));
    }

    /// 期間が短い設定では、その期間より長い前の予告は送らない。
    #[test]
    fn notices_longer_than_the_period_are_skipped() {
        assert_eq!(due_notice(days(6), 7), None);
        assert_eq!(due_notice(days(1), 7), Some(1));
        assert_eq!(due_notice(days(5), 10), Some(7));
        assert_eq!(due_notice(days(5), 1), None);
    }

    #[sqlx::test]
    async fn nothing_happens_when_disabled(pool: SqlitePool) {
        let (mailer, sent) = Mailer::recording("http://localhost:3010");
        let id = insert_user(&pool, "alice@example.com").await;
        set_last_seen(&pool, id, 1000).await;

        run_once(&pool, &mailer, 0).await.expect("回せるはず");

        assert_eq!(state(&pool, id).await, (None, None, None));
        assert!(sent.lock().expect("ロックできるはず").is_empty());
    }

    /// 予告は、時期が来るたびに1通ずつ。同じ予告は重ねて送らない。
    #[sqlx::test]
    async fn each_notice_is_sent_once(pool: SqlitePool) {
        let (mailer, sent) = Mailer::recording("http://localhost:3010");
        let id = insert_user(&pool, "alice@example.com").await;

        // 退会日まで31日: まだ送らない。
        set_last_seen(&pool, id, 334).await;
        run_once(&pool, &mailer, DELETE_AFTER)
            .await
            .expect("回せるはず");
        assert!(sent.lock().expect("ロックできるはず").is_empty());

        // 29日前: 30日前の予告。2回回しても1通。
        set_last_seen(&pool, id, 336).await;
        run_once(&pool, &mailer, DELETE_AFTER)
            .await
            .expect("回せるはず");
        run_once(&pool, &mailer, DELETE_AFTER)
            .await
            .expect("回せるはず");
        assert_eq!(sent.lock().expect("ロックできるはず").len(), 1);
        assert_eq!(state(&pool, id).await.2, Some(30));

        // 6日前: 7日前の予告。
        set_last_seen(&pool, id, 359).await;
        run_once(&pool, &mailer, DELETE_AFTER)
            .await
            .expect("回せるはず");
        assert_eq!(sent.lock().expect("ロックできるはず").len(), 2);
        assert_eq!(state(&pool, id).await.2, Some(7));
        assert_eq!(
            sent.lock().expect("ロックできるはず")[1].to,
            "alice@example.com"
        );
    }

    /// 退会日を過ぎたら削除予約を入れて知らせる。本人が使えば取り消される。
    #[sqlx::test]
    async fn schedules_deletion_after_the_deadline_and_a_visit_cancels_it(pool: SqlitePool) {
        let (mailer, sent) = Mailer::recording("http://localhost:3010");
        let id = insert_user(&pool, "alice@example.com").await;
        set_last_seen(&pool, id, 366).await;

        run_once(&pool, &mailer, DELETE_AFTER)
            .await
            .expect("回せるはず");

        let (scheduled_at, origin, _) = state(&pool, id).await;
        assert!(scheduled_at.is_some());
        assert_eq!(origin, Some(DeletionOrigin::Inactive));
        assert_eq!(sent.lock().expect("ロックできるはず").len(), 1);
        // 重ねて回しても、予約も通知も増えない。
        run_once(&pool, &mailer, DELETE_AFTER)
            .await
            .expect("回せるはず");
        assert_eq!(sent.lock().expect("ロックできるはず").len(), 1);

        account::touch_last_seen(&pool, id)
            .await
            .expect("記録できるはず");
        assert_eq!(state(&pool, id).await, (None, None, None));
    }

    /// メールが送れないアカウントも、退会は止めない。
    #[sqlx::test]
    async fn deletion_is_scheduled_even_when_no_mail_can_be_sent(pool: SqlitePool) {
        let (mailer, sent) = Mailer::recording("http://localhost:3010");
        let id = insert_user(&pool, "alice").await;
        set_last_seen(&pool, id, 400).await;

        run_once(&pool, &mailer, DELETE_AFTER)
            .await
            .expect("回せるはず");

        assert_eq!(state(&pool, id).await.1, Some(DeletionOrigin::Inactive));
        assert!(sent.lock().expect("ロックできるはず").is_empty());
    }

    #[sqlx::test]
    async fn users_with_a_remaining_topup_are_left_alone(pool: SqlitePool) {
        let (mailer, sent) = Mailer::recording("http://localhost:3010");
        let buyer = insert_user(&pool, "buyer@example.com").await;
        crate::test_support::insert_ocr_quota_grant(&pool, buyer, "cs_left", 1).await;
        set_last_seen(&pool, buyer, 1000).await;

        run_once(&pool, &mailer, DELETE_AFTER)
            .await
            .expect("回せるはず");

        assert_eq!(state(&pool, buyer).await, (None, None, None));
        assert!(sent.lock().expect("ロックできるはず").is_empty());
    }

    #[sqlx::test]
    async fn admins_and_frozen_users_are_left_alone(pool: SqlitePool) {
        let (mailer, sent) = Mailer::recording("http://localhost:3010");
        let admin = insert_user(&pool, "admin@example.com").await;
        let frozen = insert_user(&pool, "frozen@example.com").await;
        sqlx::query!("UPDATE users SET role = 'admin' WHERE id = ?", admin)
            .execute(&pool)
            .await
            .expect("管理者にできるはず");
        sqlx::query!("UPDATE users SET frozen = 1 WHERE id = ?", frozen)
            .execute(&pool)
            .await
            .expect("凍結できるはず");
        set_last_seen(&pool, admin, 1000).await;
        set_last_seen(&pool, frozen, 1000).await;

        run_once(&pool, &mailer, DELETE_AFTER)
            .await
            .expect("回せるはず");

        assert_eq!(state(&pool, admin).await, (None, None, None));
        assert_eq!(state(&pool, frozen).await, (None, None, None));
        assert!(sent.lock().expect("ロックできるはず").is_empty());
    }

    /// 前回の更新から1日たたないうちは書き込まない。たてば予告を戻す。
    #[sqlx::test]
    async fn touch_writes_at_most_once_a_day_and_resets_the_notice(pool: SqlitePool) {
        let id = insert_user(&pool, "alice@example.com").await;
        sqlx::query!(
            "UPDATE users SET inactivity_notice_days = 30 WHERE id = ?",
            id
        )
        .execute(&pool)
        .await
        .expect("予告を書けるはず");

        account::touch_last_seen(&pool, id)
            .await
            .expect("記録できるはず");
        assert_eq!(state(&pool, id).await.2, Some(30), "作成直後は書かない");

        set_last_seen(&pool, id, 2).await;
        account::touch_last_seen(&pool, id)
            .await
            .expect("記録できるはず");
        assert_eq!(state(&pool, id).await.2, None);
    }
}
