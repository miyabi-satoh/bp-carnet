//! アカウント削除フロー (docs/authentication.md)。
//!
//! 削除は即時ではなく、猶予期間を置いた予約として `users.deletion_scheduled_at` に記録する。
//! 過ぎた分は [`purge_expired_periodically`] が物理削除する。本人の削除・管理者による削除の
//! どちらもこの同じ流れを通り、違うのは取り消し方だけ ([`DeletionOrigin`])。

use std::sync::Arc;
use std::time::Duration;

use jiff::Timestamp;
use serde::Serialize;
use sqlx::{SqliteConnection, SqlitePool};
use utoipa::ToSchema;

use crate::apple_login::AppleLoginClient;
use crate::line_login::LineLoginClient;
use crate::local_time::date_label;
use crate::mail::Mailer;
use crate::oauth_identity::{Provider, RevocationTokens};
use crate::settings;

/// ADR: 猶予期間 (日数)。デザインの確認文言が「30日間の猶予期間」と
/// 書き切っているため、画面の文言と食い違わせないよう設定値にせず定数にする。
pub const DELETION_GRACE_DAYS: i64 = 30;

/// 削除予定日時を、本人のタイムゾーンでメールに書く日付へ変換する。
pub async fn local_date(
    pool: &SqlitePool,
    user_id: i64,
    at: Timestamp,
) -> Result<String, crate::error::AppError> {
    let timezone = settings::load_timezone(pool, user_id).await?;
    Ok(date_label(timezone.to_local(at).date()))
}

/// ADR: 物理削除を回す間隔。猶予期間 (30日) に対して1日の遅れは実害が無く、
/// 起動のたびに1回走らせるため、常時起動しない構成 (トレイ版) でも取りこぼさない。
const PURGE_INTERVAL: Duration = Duration::from_secs(24 * 60 * 60);

/// 削除予約の起点 (`users.deletion_origin`、docs/authentication.md)。管理者が起点の予約以外は、
/// 本人のログインで取り消される (自動退会はセッションでの操作でも、→ [`touch_last_seen`])。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, sqlx::Type, ToSchema)]
#[serde(rename_all = "lowercase")]
#[sqlx(rename_all = "lowercase")]
pub enum DeletionOrigin {
    /// 本人。
    User,
    /// 管理者。
    Admin,
    /// 自動退会 (docs/authentication.md)。
    Inactive,
}

/// 削除の予約結果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScheduledDeletion {
    /// 削除予定日時 (UTC、RFC3339)。
    pub scheduled_at: String,
    /// この呼び出しで新しく予約したなら `true`。既に予約済みなら `false` で、予定日と起点は
    /// 最初の予約のまま動かない。
    pub newly_scheduled: bool,
}

/// 予約中の削除。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingDeletion {
    /// 削除予定日時 (UTC、RFC3339)。
    pub scheduled_at: String,
    pub origin: DeletionOrigin,
}

/// `user_id` の予約中の削除。予約が無ければ `None`。
pub async fn pending_deletion(
    pool: &SqlitePool,
    user_id: i64,
) -> Result<Option<PendingDeletion>, sqlx::Error> {
    let row = sqlx::query!(
        r#"SELECT deletion_scheduled_at,
                  deletion_origin AS "deletion_origin: DeletionOrigin"
           FROM users WHERE id = ?"#,
        user_id
    )
    .fetch_one(pool)
    .await?;
    Ok(match (row.deletion_scheduled_at, row.deletion_origin) {
        (Some(scheduled_at), Some(origin)) => Some(PendingDeletion {
            scheduled_at,
            origin,
        }),
        _ => None,
    })
}

/// 削除を予約する。対象が存在しない (未確認のアカウントを含む、docs/authentication.md) なら `None`。
///
/// 既に予約済みならその予定を起点ごとそのまま返す。押し直すたびに予定日が後ろへずれると、
/// 利用者が削除されるつもりでいる日を過ぎても消えない状態になるため。
///
/// 予約と読み戻しを1つのトランザクションに収めたいので、プールではなく接続を受け取る。
/// 管理者が起点の削除は、同じトランザクションに監査ログの記録も載せる。
///
/// ADR: 存在確認を先に読み取らず、書き込みを最初の文にする。deferred なトランザクションで
/// 読みを先に置くと、後から書き込みへ昇格する際に他の接続のコミットと衝突して
/// `SQLITE_BUSY_SNAPSHOT` になりうる (busy handler は再試行しない)。
pub async fn schedule_deletion(
    conn: &mut SqliteConnection,
    user_id: i64,
    origin: DeletionOrigin,
) -> Result<Option<ScheduledDeletion>, sqlx::Error> {
    let updated = sqlx::query!(
        r#"
        UPDATE users
        SET deletion_scheduled_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now', printf('+%d days', ?)),
            deletion_origin = ?
        WHERE id = ? AND email_verified = 1 AND deletion_scheduled_at IS NULL
        "#,
        DELETION_GRACE_DAYS,
        origin,
        user_id
    )
    .execute(&mut *conn)
    .await?;

    let scheduled_at = sqlx::query_scalar!(
        r#"SELECT deletion_scheduled_at AS "deletion_scheduled_at: String" FROM users
           WHERE id = ? AND email_verified = 1"#,
        user_id
    )
    .fetch_optional(&mut *conn)
    .await?
    .flatten();

    Ok(scheduled_at.map(|scheduled_at| ScheduledDeletion {
        scheduled_at,
        newly_scheduled: updated.rows_affected() > 0,
    }))
}

/// 本人 (または自動退会) が起点の削除予約を取り消す。取り消したら `true`、無ければ `false`。
///
/// 猶予期間中の本人のログインから呼ぶ (本人向けの取り消し UI は持たない)。管理者が起点の
/// 予約は残す ([`DeletionOrigin`])。
pub async fn cancel_user_deletion(pool: &SqlitePool, user_id: i64) -> Result<bool, sqlx::Error> {
    let result = sqlx::query!(
        "UPDATE users SET deletion_scheduled_at = NULL, deletion_origin = NULL
         WHERE id = ? AND deletion_origin IN ('user', 'inactive')",
        user_id
    )
    .execute(pool)
    .await?;
    Ok(result.rows_affected() > 0)
}

/// 最終アクセスを今にし、自動退会の予告と予約を戻す (docs/authentication.md)。ログインと、
/// セッションを使った操作から呼ぶ。
///
/// 書き込みは、前回の更新から1日以上たっているか、自動退会の予約が入っているときだけ。
/// 毎回の操作で書くと、読むだけのリクエストが書き込みの競合に巻き込まれる。
pub async fn touch_last_seen(pool: &SqlitePool, user_id: i64) -> Result<(), sqlx::Error> {
    sqlx::query!(
        "UPDATE users
         SET last_seen_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now'),
             inactivity_notice_days = NULL,
             deletion_scheduled_at = CASE WHEN deletion_origin = 'inactive' THEN NULL ELSE deletion_scheduled_at END,
             deletion_origin = CASE WHEN deletion_origin = 'inactive' THEN NULL ELSE deletion_origin END
         WHERE id = ?
           AND (deletion_origin = 'inactive'
                OR COALESCE(last_seen_at, created_at) < strftime('%Y-%m-%dT%H:%M:%fZ', 'now', '-1 day'))",
        user_id
    )
    .execute(pool)
    .await?;
    Ok(())
}

/// 管理者による削除予約の取り消しの結果。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AdminDeletionCancel {
    Cancelled,
    /// 予約が無かった。取り消す対象が無いだけで、状態との衝突ではない。
    NotScheduled,
    /// 本人が起点の予約。取り消さない (docs/authentication.md)。
    ScheduledByUser,
    /// 対象が存在しない (未確認のアカウントを含む)。
    NotFound,
}

/// 管理者が起点の削除予約を取り消す。監査ログを同じトランザクションに載せるため接続を受け取る。
///
/// ADR: 書き込みを最初の文にし、結果の見分けに要る読み取りは後に置く ([`schedule_deletion`] と同じ理由)。
pub async fn cancel_admin_deletion(
    conn: &mut SqliteConnection,
    user_id: i64,
) -> Result<AdminDeletionCancel, sqlx::Error> {
    let updated = sqlx::query!(
        "UPDATE users SET deletion_scheduled_at = NULL, deletion_origin = NULL
         WHERE id = ? AND email_verified = 1 AND deletion_origin = 'admin'",
        user_id
    )
    .execute(&mut *conn)
    .await?;
    if updated.rows_affected() > 0 {
        return Ok(AdminDeletionCancel::Cancelled);
    }

    let origin = sqlx::query_scalar!(
        r#"SELECT deletion_origin AS "deletion_origin: DeletionOrigin" FROM users
           WHERE id = ? AND email_verified = 1"#,
        user_id
    )
    .fetch_optional(&mut *conn)
    .await?;
    Ok(match origin {
        None => AdminDeletionCancel::NotFound,
        Some(None) => AdminDeletionCancel::NotScheduled,
        Some(Some(DeletionOrigin::User)) => AdminDeletionCancel::ScheduledByUser,
        // 自動退会の予約も、本人が起点のものと同じく管理者は取り消さない (docs/authentication.md)。
        Some(Some(DeletionOrigin::Inactive)) => AdminDeletionCancel::ScheduledByUser,
        // 上の UPDATE で書き込みロックを取っているため、他の接続が間に予約し直すことはなく、
        // 実際には来ない。型の都合で網羅するだけ。
        Some(Some(DeletionOrigin::Admin)) => AdminDeletionCancel::NotScheduled,
    })
}

/// 猶予期間を過ぎた予約を物理削除する。削除した件数を返す。
/// `bp_records`・`oauth_identities` は `ON DELETE CASCADE` で一緒に消える。
///
/// LINE・Apple と連携していれば、消す前に連携を取り消す (LINE の開発ガイドライン・App Store 審査
/// ガイドライン 5.1.1(v) の必須事項、docs/authentication.md)。取り消せなくても削除は止めない (LINE は最後に
/// ログインしてから90日 (アプリでは30日) を過ぎると、手元のトークンでは取り消せなくなるため)。
pub async fn purge_expired(
    pool: &SqlitePool,
    line: &LineLoginClient,
    apple: &AppleLoginClient,
) -> Result<u64, sqlx::Error> {
    let due = sqlx::query_scalar!(
        r#"
        SELECT id AS "id!: i64" FROM users
        WHERE deletion_scheduled_at IS NOT NULL
          AND deletion_scheduled_at <= strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
        "#
    )
    .fetch_all(pool)
    .await?;

    let mut deleted = 0;
    for user_id in due {
        revoke_links(pool, line, apple, user_id).await?;
        // 取り消しの間に予約が取り消されていれば (本人のログイン等) 消さない。
        let result = sqlx::query!(
            r#"
            DELETE FROM users
            WHERE id = ?
              AND deletion_scheduled_at IS NOT NULL
              AND deletion_scheduled_at <= strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
            "#,
            user_id
        )
        .execute(pool)
        .await?;
        deleted += result.rows_affected();
    }
    Ok(deleted)
}

/// ユーザーの LINE・Apple との連携を取り消す。取り消せなかったことはログに残すだけにする。
async fn revoke_links(
    pool: &SqlitePool,
    line: &LineLoginClient,
    apple: &AppleLoginClient,
    user_id: i64,
) -> Result<(), sqlx::Error> {
    let rows = sqlx::query!(
        // 一覧を取ってからここまでの間に、ログインで削除が取り消されていれば取り消さない。
        r#"SELECT oauth_identities.provider, oauth_identities.refresh_token, oauth_identities.access_token
           FROM oauth_identities
           JOIN users ON users.id = oauth_identities.user_id
           WHERE oauth_identities.user_id = ?
             AND (oauth_identities.refresh_token IS NOT NULL
                  OR oauth_identities.access_token IS NOT NULL)
             AND users.deletion_scheduled_at IS NOT NULL
             AND users.deletion_scheduled_at <= strftime('%Y-%m-%dT%H:%M:%fZ', 'now')"#,
        user_id
    )
    .fetch_all(pool)
    .await?;
    for row in rows {
        let Some(provider) = Provider::parse(&row.provider) else {
            continue;
        };
        let tokens = RevocationTokens {
            refresh_token: row.refresh_token,
            access_token: row.access_token,
        };
        crate::external_login::revoke_link_logged(
            line,
            apple,
            provider,
            &tokens,
            user_id,
            "提供元との連携を取り消せませんでした。アカウントの削除は続けます",
        )
        .await;
    }
    Ok(())
}

/// 起動時に1回、以降は [`PURGE_INTERVAL`] ごとに物理削除を回す。確認リンクの期限が切れた
/// 未確認のアカウント (`crate::signup`) も同じ周期で消し、放置アカウントの予告と削除予約
/// (`crate::inactivity`) も同じ周期で判定する。失効したアプリのトークン (`crate::app_token`) と、保存期間を
/// 過ぎた購入の記録 (`crate::payments::ocr_quota`) も掃除する。
/// DB エラーでループを止めない (次の周期で回復しうるため)。
pub async fn purge_expired_periodically(
    pool: SqlitePool,
    line: Arc<LineLoginClient>,
    apple: Arc<AppleLoginClient>,
    mailer: Arc<Mailer>,
    inactive_delete_after_days: u32,
    session_expiry_days: i64,
) {
    // 処理時間ぶん周期が延びないよう、固定の間隔で回す (予告の窓は24時間ぴったりのため)。
    let mut ticker = tokio::time::interval(PURGE_INTERVAL);
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    loop {
        ticker.tick().await;
        match purge_expired(&pool, &line, &apple).await {
            Ok(0) => {}
            Ok(count) => tracing::info!(count, "猶予期間を過ぎたアカウントを削除しました"),
            Err(err) => {
                tracing::error!(error = %crate::error_chain_line(&err), "アカウントの物理削除に失敗しました")
            }
        }
        match crate::signup::purge_expired_unverified(&pool).await {
            Ok(0) => {}
            Ok(count) => tracing::info!(count, "確認されなかったアカウントを削除しました"),
            Err(err) => {
                tracing::error!(error = %crate::error_chain_line(&err), "未確認のアカウントの削除に失敗しました")
            }
        }
        if let Err(err) =
            crate::inactivity::run_once(&pool, &mailer, inactive_delete_after_days).await
        {
            tracing::error!(error = %crate::error_chain_line(&err), "放置アカウントの判定に失敗しました")
        }
        match crate::payments::ocr_quota::purge_orphaned(&pool).await {
            Ok(0) => {}
            Ok(count) => tracing::info!(count, "保存期間を過ぎた購入の記録を削除しました"),
            Err(err) => {
                tracing::error!(error = %crate::error_chain_line(&err), "購入の記録の削除に失敗しました")
            }
        }
        match crate::payments::ocr_quota::purge_usage_records(&pool).await {
            Ok(0) => {}
            Ok(count) => tracing::info!(count, "保存期間を過ぎた読み取りの記録を削除しました"),
            Err(err) => {
                tracing::error!(error = %crate::error_chain_line(&err), "読み取りの記録の削除に失敗しました")
            }
        }
        if let Err(err) = crate::app_token::purge_expired(&pool, session_expiry_days).await {
            tracing::error!(error = %crate::error_chain_line(&err), "失効したアプリのトークンの削除に失敗しました")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{conn, insert_user};

    /// 削除予約を直接書き込む (猶予期間の経過を待たずに検証するため)。
    async fn set_scheduled(pool: &SqlitePool, user_id: i64, value: &str, origin: DeletionOrigin) {
        sqlx::query!(
            "UPDATE users SET deletion_scheduled_at = ?, deletion_origin = ? WHERE id = ?",
            value,
            origin,
            user_id
        )
        .execute(pool)
        .await
        .expect("削除予定を書き換えられるはず");
    }

    async fn scheduled(
        pool: &SqlitePool,
        user_id: i64,
    ) -> (Option<String>, Option<DeletionOrigin>) {
        let row = sqlx::query!(
            r#"SELECT deletion_scheduled_at AS "deletion_scheduled_at: String",
                      deletion_origin AS "deletion_origin: DeletionOrigin"
               FROM users WHERE id = ?"#,
            user_id
        )
        .fetch_one(pool)
        .await
        .expect("ユーザーを読めるはず");
        (row.deletion_scheduled_at, row.deletion_origin)
    }

    #[sqlx::test]
    async fn schedule_deletion_sets_the_grace_period(pool: SqlitePool) {
        let user_id = insert_user(&pool, "alice").await;

        let mut conn = conn(&pool).await;
        let result = schedule_deletion(&mut conn, user_id, DeletionOrigin::Admin)
            .await
            .expect("削除を予約できるはず")
            .expect("対象が居るので予約できるはず");

        assert!(result.newly_scheduled);
        assert_eq!(
            scheduled(&pool, user_id).await,
            (
                Some(result.scheduled_at.clone()),
                Some(DeletionOrigin::Admin)
            )
        );
        // 他の日時列と同じミリ秒の形 (`...:SS.sssZ`)。
        assert_eq!(
            result.scheduled_at.len(),
            "2026-01-01T00:00:00.000Z".len(),
            "ミリ秒精度のはず (実際: {})",
            result.scheduled_at
        );
        // 予約から判定までの経過分だけ端数が出るため、切り捨てで 29 日にもなりうる。
        let days_later = sqlx::query_scalar!(
            r#"SELECT CAST(julianday(?) - julianday('now') AS INTEGER) AS "days!: i64""#,
            result.scheduled_at
        )
        .fetch_one(&pool)
        .await
        .expect("日数を求められるはず");
        assert!(
            (DELETION_GRACE_DAYS - 1..=DELETION_GRACE_DAYS).contains(&days_later),
            "削除予定は約 {DELETION_GRACE_DAYS} 日後のはず (実際: {days_later} 日後)"
        );
    }

    /// 予定日だけでなく起点も最初の予約のまま。本人が起点の予約に管理者が重ねても、本人の
    /// ログインで取り消せる状態は変わらない。
    #[sqlx::test]
    async fn schedule_deletion_keeps_the_first_schedule_and_origin(pool: SqlitePool) {
        let user_id = insert_user(&pool, "alice").await;
        set_scheduled(
            &pool,
            user_id,
            "2030-01-01T00:00:00.000Z",
            DeletionOrigin::User,
        )
        .await;

        let mut conn = conn(&pool).await;
        let result = schedule_deletion(&mut conn, user_id, DeletionOrigin::Admin)
            .await
            .expect("削除を予約できるはず")
            .expect("対象が居るので予約できるはず");

        assert_eq!(result.scheduled_at, "2030-01-01T00:00:00.000Z");
        assert!(
            !result.newly_scheduled,
            "既に予約済みなら、新しく予約したことにはならない"
        );
        drop(conn);
        assert_eq!(
            scheduled(&pool, user_id).await.1,
            Some(DeletionOrigin::User)
        );
    }

    #[sqlx::test]
    async fn schedule_deletion_reports_a_missing_user(pool: SqlitePool) {
        let mut conn = conn(&pool).await;

        assert_eq!(
            schedule_deletion(&mut conn, 9999, DeletionOrigin::User)
                .await
                .expect("クエリ自体は成功するはず"),
            None
        );
    }

    #[sqlx::test]
    async fn cancel_user_deletion_clears_a_schedule_by_the_user(pool: SqlitePool) {
        let user_id = insert_user(&pool, "alice").await;
        set_scheduled(
            &pool,
            user_id,
            "2030-01-01T00:00:00.000Z",
            DeletionOrigin::User,
        )
        .await;

        assert!(
            cancel_user_deletion(&pool, user_id)
                .await
                .expect("取り消せるはず")
        );
        assert_eq!(scheduled(&pool, user_id).await, (None, None));
    }

    #[sqlx::test]
    async fn cancel_user_deletion_keeps_a_schedule_by_an_admin(pool: SqlitePool) {
        let user_id = insert_user(&pool, "alice").await;
        set_scheduled(
            &pool,
            user_id,
            "2030-01-01T00:00:00.000Z",
            DeletionOrigin::Admin,
        )
        .await;

        assert!(
            !cancel_user_deletion(&pool, user_id)
                .await
                .expect("クエリ自体は成功するはず")
        );
        assert_eq!(
            scheduled(&pool, user_id).await,
            (
                Some("2030-01-01T00:00:00.000Z".to_string()),
                Some(DeletionOrigin::Admin)
            )
        );
    }

    #[sqlx::test]
    async fn cancel_user_deletion_reports_nothing_to_cancel(pool: SqlitePool) {
        let user_id = insert_user(&pool, "alice").await;

        assert!(
            !cancel_user_deletion(&pool, user_id)
                .await
                .expect("クエリ自体は成功するはず")
        );
    }

    #[sqlx::test]
    async fn cancel_admin_deletion_distinguishes_each_state(pool: SqlitePool) {
        let by_admin = insert_user(&pool, "by-admin").await;
        let by_user = insert_user(&pool, "by-user").await;
        let unscheduled = insert_user(&pool, "unscheduled").await;
        set_scheduled(
            &pool,
            by_admin,
            "2030-01-01T00:00:00.000Z",
            DeletionOrigin::Admin,
        )
        .await;
        set_scheduled(
            &pool,
            by_user,
            "2030-01-01T00:00:00.000Z",
            DeletionOrigin::User,
        )
        .await;

        let mut conn = conn(&pool).await;
        for (user_id, expected) in [
            (by_admin, AdminDeletionCancel::Cancelled),
            (by_user, AdminDeletionCancel::ScheduledByUser),
            (unscheduled, AdminDeletionCancel::NotScheduled),
            (9999, AdminDeletionCancel::NotFound),
        ] {
            assert_eq!(
                cancel_admin_deletion(&mut conn, user_id)
                    .await
                    .expect("クエリ自体は成功するはず"),
                expected,
                "user_id = {user_id}"
            );
        }
        drop(conn);

        assert_eq!(scheduled(&pool, by_admin).await, (None, None));
        assert_eq!(
            scheduled(&pool, by_user).await.1,
            Some(DeletionOrigin::User),
            "本人が起点の予約は残る"
        );
    }

    #[sqlx::test]
    async fn purge_expired_removes_only_users_past_the_grace_period(pool: SqlitePool) {
        let expired = insert_user(&pool, "expired").await;
        let pending = insert_user(&pool, "pending").await;
        let active = insert_user(&pool, "active").await;
        set_scheduled(
            &pool,
            expired,
            "2000-01-01T00:00:00.000Z",
            DeletionOrigin::User,
        )
        .await;
        set_scheduled(
            &pool,
            pending,
            "2100-01-01T00:00:00.000Z",
            DeletionOrigin::Admin,
        )
        .await;

        let deleted = purge_expired(
            &pool,
            &LineLoginClient::disabled(),
            &crate::apple_login::AppleLoginClient::disabled(),
        )
        .await
        .expect("物理削除できるはず");

        assert_eq!(deleted, 1);
        let remaining = sqlx::query_scalar!(r#"SELECT id AS "id!: i64" FROM users ORDER BY id"#)
            .fetch_all(&pool)
            .await
            .expect("ユーザーを読めるはず");
        assert_eq!(remaining, vec![pending, active]);
    }

    #[sqlx::test]
    async fn purge_expired_keeps_the_purchases_without_the_owner_and_remaining(pool: SqlitePool) {
        let user_id = insert_user(&pool, "buyer").await;
        crate::test_support::insert_ocr_quota_grant(&pool, user_id, "cs_kept", 50_000).await;
        set_scheduled(
            &pool,
            user_id,
            "2000-01-01T00:00:00.000Z",
            DeletionOrigin::User,
        )
        .await;

        purge_expired(
            &pool,
            &LineLoginClient::disabled(),
            &crate::apple_login::AppleLoginClient::disabled(),
        )
        .await
        .expect("物理削除できるはず");

        let row = sqlx::query!(
            "SELECT user_id, remaining_milli_yen, price_yen FROM ocr_quota_grants WHERE stripe_checkout_session_id = 'cs_kept'"
        )
        .fetch_one(&pool)
        .await
        .expect("購入の記録が残るはず");
        assert_eq!(
            (row.user_id, row.remaining_milli_yen, row.price_yen),
            (None, 0, 300)
        );
    }

    #[sqlx::test]
    async fn purge_expired_removes_the_records_of_deleted_users(pool: SqlitePool) {
        let user_id = insert_user(&pool, "expired").await;
        sqlx::query!(
            "INSERT INTO bp_records (user_id, measured_at, systolic, diastolic) VALUES (?, '2026-01-01T00:00:00Z', 120, 80)",
            user_id
        )
        .execute(&pool)
        .await
        .expect("記録を作成できるはず");
        set_scheduled(
            &pool,
            user_id,
            "2000-01-01T00:00:00.000Z",
            DeletionOrigin::User,
        )
        .await;

        purge_expired(
            &pool,
            &LineLoginClient::disabled(),
            &crate::apple_login::AppleLoginClient::disabled(),
        )
        .await
        .expect("物理削除できるはず");

        let records = sqlx::query_scalar!(r#"SELECT COUNT(*) AS "count!: i64" FROM bp_records"#)
            .fetch_one(&pool)
            .await
            .expect("件数を数えられるはず");
        assert_eq!(records, 0);
    }

    /// 自動退会の予約は、本人のログインで取り消せ、管理者は取り消せない。
    #[sqlx::test]
    async fn inactivity_deletion_is_cancelled_by_the_user_but_not_by_an_admin(pool: SqlitePool) {
        let user_id = insert_user(&pool, "alice").await;
        set_scheduled(
            &pool,
            user_id,
            "2030-01-01T00:00:00.000Z",
            DeletionOrigin::Inactive,
        )
        .await;

        let mut conn = conn(&pool).await;
        assert_eq!(
            cancel_admin_deletion(&mut conn, user_id)
                .await
                .expect("判定できるはず"),
            AdminDeletionCancel::ScheduledByUser
        );
        drop(conn);
        assert!(
            cancel_user_deletion(&pool, user_id)
                .await
                .expect("取り消せるはず")
        );
    }
}
