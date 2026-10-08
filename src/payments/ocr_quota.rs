//! 購入単位の OCR 枠。Stripe・Apple の API の形はこのモジュールに持ち込まない。
//!
//! 購入の枠は失効せず、購入の順 (`id`) に使う (docs/payments.md)。

use std::collections::HashMap;

use sqlx::{Sqlite, SqliteConnection, SqliteExecutor, SqlitePool, Transaction};

use crate::error::AppError;

/// 買い足し1回分の価格。Webhook で、Checkout の決済額がこの値かを確かめる。
pub const TOPUP_PRICE_YEN: i64 = 300;

/// アカウントを消した後も購入の記録を残す期間 (docs/payments.md)。
/// 購入した年の翌年の申告期限から7年を満たすよう、購入した年の翌年の初めから8年残す。
/// 年は日本の課税期間に合わせて日本時間で区切る (`purchased_at` は UTC)。
const RETENTION_YEARS_MODIFIER: &str = "-8 years";

/// 残りのある購入の枠1件。
pub struct Grant {
    /// 購入日時 (UTC、`now_sql` と同じ書式)。
    pub purchased_at: String,
    pub granted_milli_yen: i64,
    pub remaining_milli_yen: i64,
}

/// 残りのある購入の枠を、使う順 (購入の順) に並べる。
pub async fn available<'e, E: SqliteExecutor<'e>>(
    executor: E,
    user_id: i64,
) -> Result<Vec<Grant>, AppError> {
    let rows = sqlx::query_as!(
        Grant,
        "SELECT purchased_at, granted_milli_yen, remaining_milli_yen FROM ocr_quota_grants WHERE user_id = ? AND remaining_milli_yen > 0 ORDER BY id",
        user_id
    )
    .fetch_all(executor)
    .await?;
    Ok(rows)
}

/// 残りのある購入の枠を持つユーザーごとの、残りの合計 (1/1000円)。
pub async fn summaries(conn: &mut SqliteConnection) -> Result<HashMap<i64, i64>, AppError> {
    let rows = sqlx::query!(
        r#"SELECT user_id AS "user_id!: i64", SUM(remaining_milli_yen) AS "remaining!: i64"
           FROM ocr_quota_grants
           WHERE user_id IS NOT NULL AND remaining_milli_yen > 0
           GROUP BY user_id"#
    )
    .fetch_all(conn)
    .await?;
    Ok(rows
        .into_iter()
        .map(|row| (row.user_id, row.remaining))
        .collect())
}

/// Webhook の検証済み購入を記録する。
pub async fn grant(
    tx: &mut Transaction<'_, Sqlite>,
    user_id: i64,
    checkout_session_id: &str,
    purchased_at: &str,
    amount_milli_yen: i64,
    domestic: Option<bool>,
) -> Result<bool, AppError> {
    let result = sqlx::query!(
        "INSERT INTO ocr_quota_grants (user_id, granted_milli_yen, remaining_milli_yen, purchased_at, price_yen, stripe_checkout_session_id, domestic) VALUES (?, ?, ?, ?, ?, ?, ?) ON CONFLICT(stripe_checkout_session_id) DO NOTHING",
        user_id,
        amount_milli_yen,
        amount_milli_yen,
        purchased_at,
        TOPUP_PRICE_YEN,
        checkout_session_id,
        domestic
    )
    .execute(&mut **tx)
    .await?;
    Ok(result.rows_affected() == 1)
}

/// アプリ内課金の取引1件 (Apple から取り直したもの)。
pub struct AppStorePurchase<'a> {
    /// `appAccountToken` の持ち主。行が既にあれば、行の持ち主のまま変えない。
    pub user_id: i64,
    pub transaction_id: &'a str,
    /// `Production`・`Sandbox`。
    pub environment: &'a str,
    pub purchased_at: &'a str,
    /// 返金・取り消し済み (`revocationDate` がある)。
    pub revoked: bool,
    /// 取引の `signedDate` (ミリ秒)。行に残したものより新しいときだけ行を変える。
    pub signed_date: i64,
    /// 取引の storefront が日本か。行を作るときだけ使う。
    pub domestic: Option<bool>,
    /// 行を作るときに付ける量 (1/1000円、`crate::config::OcrCosts`)。
    pub granted_milli_yen: i64,
}

/// [`apply_app_store_purchase`] で行に起きたこと。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppStorePurchaseChange {
    /// 残りのある行を作った。
    Granted,
    /// 取り消した行の残りを戻した。
    Restored,
    /// 残りのある行が既にあり、何もしなかった。
    AlreadyActive,
    /// 取り消した (残り0の行を作った、または残りを `revoked_milli_yen` に移した)。
    Revoked,
    /// 取り消した行が既にあり、何もしなかった (行より古い有効な取引が届いたときも)。
    AlreadyRevoked,
}

/// アプリ内課金の取引を、今の行の状態から1つだけ反映する (docs/payments.md)。
/// `revoked_milli_yen` が空でないことが、取り消した行の印。行に残した `signedDate` より古い取引では
/// 行を変えない (取り直してから書くまでの間に、別の経路が新しい状態を書いていることがあるため)。
pub async fn apply_app_store_purchase(
    tx: &mut Transaction<'_, Sqlite>,
    purchase: &AppStorePurchase<'_>,
) -> Result<AppStorePurchaseChange, AppError> {
    let row = sqlx::query!(
        r#"SELECT id AS "id!: i64", revoked_milli_yen, apple_signed_date AS "apple_signed_date!: i64"
           FROM ocr_quota_grants WHERE apple_transaction_id = ?"#,
        purchase.transaction_id
    )
    .fetch_optional(&mut **tx)
    .await?;
    let Some(row) = row else {
        // 取り消し済みでも、残り0の行を作っておく。後から届いた付与が、行があるので何もしないように。
        let (remaining, revoked_milli_yen) = if purchase.revoked {
            (0, Some(purchase.granted_milli_yen))
        } else {
            (purchase.granted_milli_yen, None)
        };
        sqlx::query!(
            "INSERT INTO ocr_quota_grants (user_id, granted_milli_yen, remaining_milli_yen, purchased_at, price_yen, apple_transaction_id, apple_environment, revoked_milli_yen, apple_signed_date, domestic) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
            purchase.user_id,
            purchase.granted_milli_yen,
            remaining,
            purchase.purchased_at,
            TOPUP_PRICE_YEN,
            purchase.transaction_id,
            purchase.environment,
            revoked_milli_yen,
            purchase.signed_date,
            purchase.domestic
        )
        .execute(&mut **tx)
        .await?;
        return Ok(if purchase.revoked {
            AppStorePurchaseChange::Revoked
        } else {
            AppStorePurchaseChange::Granted
        });
    };
    let row_revoked = row.revoked_milli_yen.is_some();
    if purchase.signed_date <= row.apple_signed_date {
        return Ok(if row_revoked {
            AppStorePurchaseChange::AlreadyRevoked
        } else {
            AppStorePurchaseChange::AlreadyActive
        });
    }
    let change = match (row_revoked, purchase.revoked) {
        (false, true) => {
            sqlx::query!(
                "UPDATE ocr_quota_grants SET revoked_milli_yen = remaining_milli_yen, remaining_milli_yen = 0, apple_signed_date = ? WHERE id = ?",
                purchase.signed_date,
                row.id
            )
            .execute(&mut **tx)
            .await?;
            AppStorePurchaseChange::Revoked
        }
        (true, false) => {
            sqlx::query!(
                "UPDATE ocr_quota_grants SET remaining_milli_yen = revoked_milli_yen, revoked_milli_yen = NULL, apple_signed_date = ? WHERE id = ?",
                purchase.signed_date,
                row.id
            )
            .execute(&mut **tx)
            .await?;
            AppStorePurchaseChange::Restored
        }
        // 状態は変わらないが、これより古い取引で変えないよう、新しい時刻を残す。
        (row_revoked, _) => {
            sqlx::query!(
                "UPDATE ocr_quota_grants SET apple_signed_date = ? WHERE id = ?",
                purchase.signed_date,
                row.id
            )
            .execute(&mut **tx)
            .await?;
            if row_revoked {
                AppStorePurchaseChange::AlreadyRevoked
            } else {
                AppStorePurchaseChange::AlreadyActive
            }
        }
    };
    Ok(change)
}

/// 持ち主が消えた購入の記録 (アプリ内課金) の、取り消した印を取引に合わせる。残りは 0 のまま
/// (未使用分はアカウントと一緒に消えている)。行に残した `signedDate` より古い取引では変えない。
/// その取引の、持ち主の無い行があれば `true`。
pub async fn sync_orphaned_app_store_purchase(
    pool: &SqlitePool,
    transaction_id: &str,
    revoked: bool,
    signed_date: i64,
) -> Result<bool, AppError> {
    let mut tx = pool.begin().await?;
    sqlx::query!(
        "UPDATE ocr_quota_grants
         SET revoked_milli_yen = CASE WHEN ? THEN COALESCE(revoked_milli_yen, remaining_milli_yen) END,
             remaining_milli_yen = 0, apple_signed_date = ?
         WHERE apple_transaction_id = ? AND user_id IS NULL AND apple_signed_date < ?",
        revoked,
        signed_date,
        transaction_id,
        signed_date
    )
    .execute(&mut *tx)
    .await?;
    let exists = sqlx::query_scalar!(
        r#"SELECT EXISTS (SELECT 1 FROM ocr_quota_grants WHERE apple_transaction_id = ? AND user_id IS NULL) AS "exists!: bool""#,
        transaction_id
    )
    .fetch_one(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(exists)
}

/// 持ち主が消えた購入の記録のうち、保存期間を過ぎた行を消す。消した件数を返す。
pub async fn purge_orphaned(pool: &SqlitePool) -> Result<u64, sqlx::Error> {
    let result = sqlx::query!(
        "DELETE FROM ocr_quota_grants WHERE user_id IS NULL AND purchased_at < strftime('%Y-%m-%dT%H:%M:%fZ', 'now', '+9 hours', 'start of year', ?, '-9 hours')",
        RETENTION_YEARS_MODIFIER
    )
    .execute(pool)
    .await?;
    Ok(result.rows_affected())
}

/// 保存期間 (購入の記録と同じ) を過ぎた、読み取りに使った額の記録を消す。消した件数を返す。
pub async fn purge_usage_records(pool: &SqlitePool) -> Result<u64, sqlx::Error> {
    let result = sqlx::query!(
        "DELETE FROM ocr_usage_records WHERE used_at < strftime('%Y-%m-%dT%H:%M:%fZ', 'now', '+9 hours', 'start of year', ?, '-9 hours')",
        RETENTION_YEARS_MODIFIER
    )
    .execute(pool)
    .await?;
    Ok(result.rows_affected())
}

/// 購入の枠から購入の順に減らし、減らした分を購入ごとに `ocr_usage_records` に残す。減らした合計を返す。
/// `used_at` は読み取りの時刻 (`now_sql` の書式)。同じ読み取りの無料の分 (`record_free_use`) とそろえる。
pub async fn consume(
    tx: &mut Transaction<'_, Sqlite>,
    user_id: i64,
    amount_milli_yen: i64,
    used_at: &str,
) -> Result<i64, AppError> {
    if amount_milli_yen <= 0 {
        return Ok(0);
    }
    let rows = sqlx::query!(
        "SELECT id, remaining_milli_yen, granted_milli_yen, price_yen, apple_environment, domestic FROM ocr_quota_grants WHERE user_id = ? AND remaining_milli_yen > 0 ORDER BY id",
        user_id
    )
    .fetch_all(&mut **tx)
    .await?;
    let mut left = amount_milli_yen;
    let mut consumed = 0;
    for row in rows {
        let id = row.id;
        let remaining = row.remaining_milli_yen;
        if left == 0 {
            break;
        }
        let take = remaining.min(left);
        let result = sqlx::query!(
            "UPDATE ocr_quota_grants SET remaining_milli_yen = remaining_milli_yen - ? WHERE id = ? AND remaining_milli_yen >= ?",
            take,
            id,
            take
        )
        .execute(&mut **tx)
        .await?;
        if result.rows_affected() == 1 {
            consumed += take;
            left -= take;
            let (kind, domestic, sales_milli_yen) =
                if row.apple_environment.as_deref() == Some("Sandbox") {
                    ("sandbox", None, 0)
                } else {
                    (
                        "paid",
                        row.domestic,
                        sales_milli_yen(take, row.price_yen, row.granted_milli_yen),
                    )
                };
            sqlx::query!(
                "INSERT INTO ocr_usage_records (used_at, kind, grant_id, domestic, used_milli_yen, sales_milli_yen) VALUES (?, ?, ?, ?, ?, ?)",
                used_at,
                kind,
                id,
                domestic,
                take,
                sales_milli_yen
            )
            .execute(&mut **tx)
            .await?;
        }
    }
    Ok(consumed)
}

/// 購入の枠から使った量を、その購入の売値に直した額 (1/1000円、税込み、切り捨て)。
fn sales_milli_yen(used_milli_yen: i64, price_yen: i64, granted_milli_yen: i64) -> i64 {
    used_milli_yen * price_yen * 1000 / granted_milli_yen
}

/// 無料の枠から使った量を `ocr_usage_records` に残す。
pub async fn record_free_use(
    tx: &mut Transaction<'_, Sqlite>,
    used_milli_yen: i64,
    used_at: &str,
) -> Result<(), AppError> {
    sqlx::query!(
        "INSERT INTO ocr_usage_records (used_at, kind, used_milli_yen, sales_milli_yen) VALUES (?, 'free', ?, 0)",
        used_at,
        used_milli_yen
    )
    .execute(&mut **tx)
    .await?;
    Ok(())
}

/// 返金・不審請求の申し立てで、購入の残りを消し、取り消した印 (`revoked_milli_yen`) を付ける。
/// アカウントを消した後の購入の記録 (持ち主が NULL) にも印を付ける。
pub async fn revoke_unspent(
    tx: &mut Transaction<'_, Sqlite>,
    checkout_session_id: &str,
) -> Result<(), AppError> {
    sqlx::query!(
        "UPDATE ocr_quota_grants SET revoked_milli_yen = remaining_milli_yen, remaining_milli_yen = 0 WHERE stripe_checkout_session_id = ? AND revoked_milli_yen IS NULL",
        checkout_session_id
    )
    .execute(&mut **tx)
    .await?;
    Ok(())
}

/// 指定した Checkout Session の grant が (このユーザーのものとして) 既にあるか。
/// 購入結果の確認 (`GET /payments/ocr-topup/result`) で、Webhook 処理済みかどうかを
/// Stripe を呼ばずに判定するために使う。
pub async fn grant_exists(
    pool: &SqlitePool,
    user_id: i64,
    checkout_session_id: &str,
) -> Result<bool, AppError> {
    let exists = sqlx::query_scalar!(
        r#"SELECT COUNT(*) AS "value!: i64"
           FROM ocr_quota_grants WHERE user_id = ? AND stripe_checkout_session_id = ?"#,
        user_id,
        checkout_session_id
    )
    .fetch_one(pool)
    .await?;
    Ok(exists > 0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{
        TEST_TOPUP_GRANT_MILLI_YEN, insert_ocr_quota_grant, insert_user, paid_remaining,
    };

    #[sqlx::test]
    async fn orphaned_purchases_are_kept_until_the_retention_period_ends(pool: SqlitePool) {
        let user_id = insert_user(&pool, "gone").await;
        insert_ocr_quota_grant(&pool, user_id, "old", 0).await;
        insert_ocr_quota_grant(&pool, user_id, "recent", 0).await;
        insert_ocr_quota_grant(&pool, user_id, "owned", 0).await;
        // 8年前の年の初めより前の購入と、去年の購入。
        sqlx::query!(
            "UPDATE ocr_quota_grants SET purchased_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now', 'start of year', '-8 years', '-1 day') WHERE stripe_checkout_session_id IN ('old', 'owned')"
        )
        .execute(&pool)
        .await
        .expect("購入日時を書き換えられるはず");
        sqlx::query!(
            "UPDATE ocr_quota_grants SET purchased_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now', '-1 year'), user_id = NULL WHERE stripe_checkout_session_id = 'recent'"
        )
        .execute(&pool)
        .await
        .expect("購入日時を書き換えられるはず");
        sqlx::query!(
            "UPDATE ocr_quota_grants SET user_id = NULL WHERE stripe_checkout_session_id = 'old'"
        )
        .execute(&pool)
        .await
        .expect("持ち主を外せるはず");

        assert_eq!(purge_orphaned(&pool).await.expect("消せるはず"), 1);

        let left = sqlx::query_scalar!(
            r#"SELECT stripe_checkout_session_id AS "id!: String" FROM ocr_quota_grants ORDER BY id"#
        )
        .fetch_all(&pool)
        .await
        .expect("読めるはず");
        assert_eq!(left, vec!["recent".to_string(), "owned".to_string()]);
    }

    /// 読み取りに使った額の記録は、使った年の翌年の初めから8年を過ぎたものだけ消す。
    #[sqlx::test]
    async fn usage_records_are_kept_until_the_retention_period_ends(pool: SqlitePool) {
        sqlx::query!(
            "INSERT INTO ocr_usage_records (used_at, kind, used_milli_yen, sales_milli_yen) VALUES
                 (strftime('%Y-%m-%dT%H:%M:%fZ', 'now', 'start of year', '-8 years', '-1 day'), 'free', 1, 0),
                 (strftime('%Y-%m-%dT%H:%M:%fZ', 'now', '-1 year'), 'free', 2, 0)"
        )
        .execute(&pool)
        .await
        .expect("記録を入れられるはず");

        assert_eq!(purge_usage_records(&pool).await.expect("消せるはず"), 1);

        let left = sqlx::query_scalar!("SELECT used_milli_yen FROM ocr_usage_records")
            .fetch_all(&pool)
            .await
            .expect("読めるはず");
        assert_eq!(left, vec![2]);
    }

    #[sqlx::test]
    async fn a_refund_after_the_account_is_deleted_marks_the_purchase_revoked(pool: SqlitePool) {
        let user_id = insert_user(&pool, "refunded").await;
        insert_ocr_quota_grant(&pool, user_id, "cs_refunded", 30_000).await;
        sqlx::query!("DELETE FROM users WHERE id = ?", user_id)
            .execute(&pool)
            .await
            .expect("消せるはず");

        let mut tx = pool.begin().await.expect("transaction should start");
        revoke_unspent(&mut tx, "cs_refunded")
            .await
            .expect("取り消せるはず");
        tx.commit().await.expect("transaction should commit");

        let row = sqlx::query!(
            "SELECT user_id, remaining_milli_yen, revoked_milli_yen FROM ocr_quota_grants WHERE stripe_checkout_session_id = 'cs_refunded'"
        )
        .fetch_one(&pool)
        .await
        .expect("読めるはず");
        assert_eq!(
            (row.user_id, row.remaining_milli_yen, row.revoked_milli_yen),
            (None, 0, Some(0))
        );
    }

    #[sqlx::test]
    async fn consumes_grants_in_purchase_order(pool: SqlitePool) {
        let user_id = insert_user(&pool, "quota-user").await;
        insert_ocr_quota_grant(&pool, user_id, "earlier", 200).await;
        insert_ocr_quota_grant(&pool, user_id, "later", 100).await;

        let mut tx = pool.begin().await.expect("transaction should start");
        assert_eq!(
            consume(&mut tx, user_id, 250, "2026-01-01T00:00:00.000Z")
                .await
                .expect("consume should work"),
            250
        );
        tx.commit().await.expect("transaction should commit");

        let rows = sqlx::query!(
            "SELECT stripe_checkout_session_id, remaining_milli_yen FROM ocr_quota_grants WHERE user_id = ? ORDER BY id",
            user_id
        )
        .fetch_all(&pool)
        .await
        .expect("grants should be readable");
        assert_eq!(
            rows.iter()
                .map(|row| (
                    row.stripe_checkout_session_id.clone(),
                    row.remaining_milli_yen
                ))
                .collect::<Vec<_>>(),
            vec![
                (Some("earlier".to_string()), 0),
                (Some("later".to_string()), 50)
            ]
        );
    }

    /// 同時に複数の読み取りが同じ grant を消費しても、残高が負数にならず、コミットできた分の
    /// 消費量と残高の合計が元の付与額を超えない。
    ///
    /// `consume` は先に対象行を `SELECT` してから `UPDATE` するため、`account::schedule_deletion`
    /// 等の他の書き込みが「先に書いてから読み戻す」順にしている ADR (`SQLITE_BUSY_SNAPSHOT` は
    /// busy handler が再試行しない) の対象になりうる。金額が確定してから引き落とす他の書き込みと
    /// 異なり、`consume` はどの grant から幾ら引くかが読み取った残高に依存するため、書き込みを
    /// 先に置く形にはできない。そのため同時実行時の一部の attempt が `SQLITE_BUSY_SNAPSHOT` で
    /// 失敗すること自体は許容し (呼び出し元は失敗した読み取りとして扱われ、利用者は結果的に
    /// 再試行できる)、コミットできた attempt の範囲で残高が負数にならず二重消費も無いことを検証する。
    #[sqlx::test]
    async fn concurrent_consumption_never_goes_negative(pool: SqlitePool) {
        let user_id = insert_user(&pool, "concurrent-user").await;
        const GRANTED: i64 = 100;
        insert_ocr_quota_grant(&pool, user_id, "concurrent", GRANTED).await;

        // 需要の合計 (30 * 5 = 150) が付与額 (100) を上回るようにして、取り合いを起こす。
        const ATTEMPTS: i64 = 5;
        const AMOUNT_PER_ATTEMPT: i64 = 30;
        let mut tasks = Vec::new();
        for _ in 0..ATTEMPTS {
            let pool = pool.clone();
            tasks.push(tokio::spawn(async move {
                let mut tx = pool.begin().await.expect("transaction should start");
                match consume(
                    &mut tx,
                    user_id,
                    AMOUNT_PER_ATTEMPT,
                    "2026-01-01T00:00:00.000Z",
                )
                .await
                {
                    Ok(consumed) => tx.commit().await.is_ok().then_some(consumed),
                    Err(_) => None,
                }
            }));
        }

        let mut total_consumed = 0;
        let mut succeeded = 0;
        for task in tasks {
            if let Some(consumed) = task.await.expect("task should not panic") {
                total_consumed += consumed;
                succeeded += 1;
            }
        }
        assert!(
            succeeded > 0,
            "同時実行の全滅は想定外 (競合の起こし方を見直す)"
        );

        let final_remaining = paid_remaining(&pool, user_id).await;
        assert!(final_remaining >= 0, "残高が負数になってはいけない");
        assert_eq!(
            total_consumed + final_remaining,
            GRANTED,
            "コミットできた消費量と残高の合計は、元の付与額と一致する (二重消費も取りこぼしも無い)"
        );
        assert!(
            total_consumed <= GRANTED,
            "コミットできた消費量が付与額を超えてはいけない"
        );
    }

    #[sqlx::test]
    async fn the_checkout_session_is_idempotent(pool: SqlitePool) {
        let user_id = insert_user(&pool, "idempotent-user").await;
        let mut tx = pool.begin().await.expect("transaction should start");
        assert!(
            grant(
                &mut tx,
                user_id,
                "same-session",
                "2026-01-01T00:00:00.000Z",
                TEST_TOPUP_GRANT_MILLI_YEN,
                Some(true),
            )
            .await
            .expect("first grant should work")
        );
        assert!(
            !grant(
                &mut tx,
                user_id,
                "same-session",
                "2026-01-01T00:00:00.000Z",
                TEST_TOPUP_GRANT_MILLI_YEN,
                Some(true),
            )
            .await
            .expect("duplicate grant should be ignored")
        );
        tx.commit().await.expect("transaction should commit");
        assert_eq!(
            paid_remaining(&pool, user_id).await,
            TEST_TOPUP_GRANT_MILLI_YEN
        );
    }
}
