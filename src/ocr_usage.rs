//! OCR の累計金額の記録と上限判定 (docs/ocr.md)。
//! 読み取りの枠 (無料の枠と購入の枠) は、呼び出しの前に残りがあるかだけを見て、後から実際の額を引く。
//! 同じユーザーの読み取りは1件ずつにする (`InFlight`)。

use std::collections::HashSet;
use std::sync::{Arc, Mutex};

use sqlx::{SqliteExecutor, SqlitePool};

use crate::error::AppError;
use crate::payments::ocr_quota;
/// 読み取り中のユーザー。同じユーザーの読み取りを1件ずつにする。
///
/// ADR: 枠の残りは呼ぶ前に「残りがあるか」だけを見て、呼んだ後で引く。同時に呼ばれると、その数だけ
/// 残りを超えて使える。1件ずつにして超え幅を1回分に抑える。本番はマシン1台なので、
/// メモリに持つだけで足りる。
#[derive(Default)]
pub struct InFlight(Mutex<HashSet<i64>>);

impl InFlight {
    /// 読み取りを始める。同じユーザーの読み取りが終わっていなければ `AppError::OcrInProgress`。
    /// 返した印を捨てると (応答を返した・接続が切れて中断された)、終わったことになる。
    pub fn begin(self: &Arc<Self>, user_id: i64) -> Result<InFlightGuard, AppError> {
        let mut users = self.0.lock().unwrap_or_else(|e| e.into_inner());
        if !users.insert(user_id) {
            return Err(AppError::OcrInProgress);
        }
        Ok(InFlightGuard {
            in_flight: Arc::clone(self),
            user_id,
        })
    }
}

/// `InFlight::begin` の印。
pub struct InFlightGuard {
    in_flight: Arc<InFlight>,
    user_id: i64,
}

impl Drop for InFlightGuard {
    fn drop(&mut self) {
        let mut users = self.in_flight.0.lock().unwrap_or_else(|e| e.into_inner());
        users.remove(&self.user_id);
    }
}

/// 読み取りの枠の種類。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum QuotaKind {
    /// 購入の枠。購入日時 (UTC、`now_sql` と同じ書式) を持つ。
    Paid { purchased_at: String },
    /// 無料の枠 (累計の金額の上限 − 使った累計)。
    Free,
}

/// 読み取りの枠1つ。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Quota {
    pub kind: QuotaKind,
    pub size_milli_yen: i64,
    pub remaining_milli_yen: i64,
}

impl Quota {
    /// 残りの割合 (%)。使い切りだけが 0 で、残りが少しでもあれば 1 以上になるよう切り上げる
    /// (画面の「残り 0%」は使い切りだけを指す、docs/ocr.md)。
    pub fn remaining_percent(&self) -> i64 {
        if self.remaining_milli_yen <= 0 {
            return 0;
        }
        if self.size_milli_yen <= 0 {
            return 100;
        }
        let remaining = i128::from(self.remaining_milli_yen);
        let size = i128::from(self.size_milli_yen);
        let percent = (remaining * 100 + size - 1) / size;
        i64::try_from(percent.min(100)).unwrap_or(100)
    }
}

/// このユーザーの読み取りの枠を、使う順 (購入の枠を買った順、無料の枠は最後) に並べる。
/// 無料の枠が無制限なら `None` (枠を見ない。購入の枠にも手を付けない)。
///
/// 購入の枠は残りのあるものだけ、無料の枠は上限が 0 円でなければ使い切っていても並べる。
/// 画面・残りの判定・使った額の引き落としは、すべてこの並びに従う (docs/ocr.md)。
pub async fn quotas(
    pool: &SqlitePool,
    user_id: i64,
    default_budget_yen: i64,
) -> Result<Option<Vec<Quota>>, AppError> {
    let Some(budget_yen) = budget_for(pool, user_id, default_budget_yen).await? else {
        return Ok(None);
    };
    let mut quotas: Vec<Quota> = ocr_quota::available(pool, user_id)
        .await?
        .into_iter()
        .map(|grant| Quota {
            kind: QuotaKind::Paid {
                purchased_at: grant.purchased_at,
            },
            size_milli_yen: grant.granted_milli_yen,
            remaining_milli_yen: grant.remaining_milli_yen,
        })
        .collect();
    if budget_yen > 0 {
        let size = budget_yen.saturating_mul(1000);
        // 無料の枠の累計は上限を超えうる (無制限の間に積もった分・最後の1回の超過) ので、0で止める。
        let spent = spent_milli_yen(pool, user_id).await?;
        quotas.push(Quota {
            kind: QuotaKind::Free,
            size_milli_yen: size,
            remaining_milli_yen: (size - spent).max(0),
        });
    }
    Ok(Some(quotas))
}

/// 全部の枠を使い切っていれば `AppError::OcrBudgetExhausted`。無料の枠が無制限なら通す。
///
/// 判定と引き落としは別の文なので、同時に走った呼び出しの分だけ残りを超えうる (`InFlight` が超え幅を抑える)。
pub async fn ensure_budget(
    pool: &SqlitePool,
    user_id: i64,
    default_budget_yen: i64,
) -> Result<(), AppError> {
    match quotas(pool, user_id, default_budget_yen).await? {
        Some(quotas) if quotas.iter().all(|quota| quota.remaining_milli_yen <= 0) => {
            Err(AppError::OcrBudgetExhausted)
        }
        _ => Ok(()),
    }
}

/// 読み取りに使った額を記録する (使うたびの記録 `ocr_usage_records` にも残す)。無料の枠に上限があれば、`quotas` の並びどおり購入の枠を先に使い
/// (買った分がすぐ効くため、docs/ocr.md)、足りない分を無料の枠の累計に足す。
/// 無制限なら全額を累計に足し、購入の枠には手を付けない。
pub async fn add_spent(
    pool: &SqlitePool,
    user_id: i64,
    milli_yen: i64,
    default_budget_yen: i64,
) -> Result<(), AppError> {
    let milli_yen = milli_yen.max(0);
    let used_at = crate::payments::now_sql();
    let mut tx = pool.begin().await?;
    let paid_use = if budget_for(&mut *tx, user_id, default_budget_yen)
        .await?
        .is_some()
    {
        ocr_quota::consume(&mut tx, user_id, milli_yen, &used_at).await?
    } else {
        0
    };
    let free_use = milli_yen - paid_use;
    if free_use > 0 {
        ocr_quota::record_free_use(&mut tx, free_use, &used_at).await?;
        sqlx::query!(
            "UPDATE users SET ocr_spent_milli_yen = ocr_spent_milli_yen + ? WHERE id = ?",
            free_use,
            user_id
        )
        .execute(&mut *tx)
        .await?;
    }
    tx.commit().await?;
    Ok(())
}

/// このユーザーに適用される累計の上限 (円)。`users.ocr_budget_yen` を優先し、`NULL` なら
/// `default_budget_yen`。負値は無制限で `None`。
async fn budget_for<'e, E: SqliteExecutor<'e>>(
    executor: E,
    user_id: i64,
    default_budget_yen: i64,
) -> Result<Option<i64>, AppError> {
    let budget_yen = sqlx::query_scalar!("SELECT ocr_budget_yen FROM users WHERE id = ?", user_id)
        .fetch_one(executor)
        .await?
        .unwrap_or(default_budget_yen);
    Ok((budget_yen >= 0).then_some(budget_yen))
}

/// 無料の枠の累計 (1/1000円)。
async fn spent_milli_yen<'e, E: SqliteExecutor<'e>>(
    executor: E,
    user_id: i64,
) -> Result<i64, AppError> {
    let spent = sqlx::query_scalar!(
        "SELECT ocr_spent_milli_yen FROM users WHERE id = ?",
        user_id
    )
    .fetch_one(executor)
    .await?;
    Ok(spent)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{
        TEST_TOPUP_GRANT_MILLI_YEN, insert_ocr_quota_grant, insert_user, paid_remaining,
    };

    /// 無料の枠の累計を直接書き換える (`add_spent` の枠の振り分けを経ずに、使った額を用意する)。
    async fn set_free_spent(pool: &SqlitePool, user_id: i64, milli_yen: i64) {
        sqlx::query!(
            "UPDATE users SET ocr_spent_milli_yen = ? WHERE id = ?",
            milli_yen,
            user_id
        )
        .execute(pool)
        .await
        .expect("failed to set spent");
    }

    async fn spent(pool: &SqlitePool, user_id: i64) -> i64 {
        spent_milli_yen(pool, user_id)
            .await
            .expect("failed to read spent")
    }

    /// 各枠の残りの割合を使う順に。無制限なら `None`。
    async fn percents(
        pool: &SqlitePool,
        user_id: i64,
        default_budget_yen: i64,
    ) -> Option<Vec<i64>> {
        quotas(pool, user_id, default_budget_yen)
            .await
            .expect("failed to list quotas")
            .map(|quotas| quotas.iter().map(Quota::remaining_percent).collect())
    }

    async fn set_budget(pool: &SqlitePool, user_id: i64, budget_yen: Option<i64>) {
        sqlx::query!(
            "UPDATE users SET ocr_budget_yen = ? WHERE id = ?",
            budget_yen,
            user_id
        )
        .execute(pool)
        .await
        .expect("failed to set the budget");
    }

    #[sqlx::test]
    async fn the_budget_blocks_once_the_spent_amount_reaches_it(pool: SqlitePool) {
        let user_id = insert_user(&pool, "kyoko").await;
        // 既定が無制限で、個別の指定も無ければ、いくら使っていても通る。
        set_free_spent(&pool, user_id, 1_000_000).await;
        ensure_budget(&pool, user_id, -1)
            .await
            .expect("unlimited should pass");

        // 既定が 2円なら、1.999円までは通り、2円に達すると弾く。
        set_free_spent(&pool, user_id, 1_999).await;
        ensure_budget(&pool, user_id, 2)
            .await
            .expect("under the budget should pass");
        set_free_spent(&pool, user_id, 2_000).await;
        let err = ensure_budget(&pool, user_id, 2)
            .await
            .expect_err("at the budget should be rejected");
        assert!(matches!(err, AppError::OcrBudgetExhausted));
    }

    #[sqlx::test]
    async fn the_per_user_budget_overrides_the_default(pool: SqlitePool) {
        let user_id = insert_user(&pool, "kyoko").await;
        set_free_spent(&pool, user_id, 5_000).await;

        // 個別の 10円が、既定の 1円より優先される。
        set_budget(&pool, user_id, Some(10)).await;
        ensure_budget(&pool, user_id, 1)
            .await
            .expect("the per-user budget should win");
        // 0 は使えない。
        set_budget(&pool, user_id, Some(0)).await;
        assert!(matches!(
            ensure_budget(&pool, user_id, -1).await,
            Err(AppError::OcrBudgetExhausted)
        ));
        // 負値は、既定が絞っていても無制限。
        set_budget(&pool, user_id, Some(-1)).await;
        ensure_budget(&pool, user_id, 1)
            .await
            .expect("negative should be unlimited");
    }

    /// 無料の枠が0円なら無料の枠は並べず、購入の枠が残っていれば使い切り扱いにしない。
    #[sqlx::test]
    async fn zero_free_budget_with_a_paid_topup_remaining_does_not_panic(pool: SqlitePool) {
        let user_id = insert_user(&pool, "kyoko").await;
        set_budget(&pool, user_id, Some(0)).await;

        insert_ocr_quota_grant(
            &pool,
            user_id,
            "cs_zero_budget_topup",
            TEST_TOPUP_GRANT_MILLI_YEN,
        )
        .await;

        assert_eq!(percents(&pool, user_id, -1).await, Some(vec![100]));
        ensure_budget(&pool, user_id, -1)
            .await
            .expect("paid quota should allow use even with a zero free budget");
    }

    /// 無料枠が無制限なら、使った額は全部無料枠の累計に足し、買い足し枠には手を付けない。
    #[sqlx::test]
    async fn unlimited_free_budget_records_the_spent_amount_and_keeps_the_topup(pool: SqlitePool) {
        let user_id = insert_user(&pool, "kyoko").await;
        insert_ocr_quota_grant(
            &pool,
            user_id,
            "cs_unlimited_topup",
            TEST_TOPUP_GRANT_MILLI_YEN,
        )
        .await;

        add_spent(&pool, user_id, 1_300, -1)
            .await
            .expect("failed to add");

        assert_eq!(spent(&pool, user_id).await, 1_300);
        assert_eq!(
            paid_remaining(&pool, user_id).await,
            TEST_TOPUP_GRANT_MILLI_YEN
        );
    }

    /// 無料の枠に上限があれば、購入の枠を先に使い、足りない分だけ無料の枠の累計に足す。
    #[sqlx::test]
    async fn the_topup_is_spent_before_the_free_budget(pool: SqlitePool) {
        let user_id = insert_user(&pool, "kyoko").await;
        insert_ocr_quota_grant(&pool, user_id, "cs_spent_first", 1_000).await;
        let topup = || paid_remaining(&pool, user_id);

        add_spent(&pool, user_id, 600, 50)
            .await
            .expect("failed to add");
        assert_eq!((spent(&pool, user_id).await, topup().await), (0, 400));

        add_spent(&pool, user_id, 600, 50)
            .await
            .expect("failed to add");
        assert_eq!((spent(&pool, user_id).await, topup().await), (200, 0));
    }

    /// 無料枠の累計が上限を超えていても (無制限の間に積もった・最後の1回で超えた)、
    /// 超えた分で買い足し枠を減らさない。
    #[sqlx::test]
    async fn spending_over_the_free_budget_does_not_eat_into_the_topup(pool: SqlitePool) {
        let user_id = insert_user(&pool, "kyoko").await;
        set_free_spent(&pool, user_id, 200_000).await;
        insert_ocr_quota_grant(&pool, user_id, "cs_over_budget", TEST_TOPUP_GRANT_MILLI_YEN).await;

        assert_eq!(percents(&pool, user_id, 50).await, Some(vec![100, 0]));
        ensure_budget(&pool, user_id, 50)
            .await
            .expect("the topup should be usable");
    }

    /// 残りの割合は切り上げ、使い切りだけが 0、無制限は None。
    #[sqlx::test]
    async fn the_remaining_percent_rounds_up_and_zero_means_exhausted(pool: SqlitePool) {
        let user_id = insert_user(&pool, "kyoko").await;

        // 個別の指定が無ければ既定 (50円) に対する割合。
        assert_eq!(percents(&pool, user_id, 50).await, Some(vec![100]));
        set_free_spent(&pool, user_id, 20_000).await; // 40% 使用
        assert_eq!(percents(&pool, user_id, 50).await, Some(vec![60]));
        set_free_spent(&pool, user_id, 49_999).await; // 残り 0.001円: 切り上げて 1%
        assert_eq!(percents(&pool, user_id, 50).await, Some(vec![1]));
        set_free_spent(&pool, user_id, 50_000).await;
        assert_eq!(percents(&pool, user_id, 50).await, Some(vec![0]));
        // 上限0 (無料の枠が無い) は枠を並べない、負値 (無制限) は None。
        assert_eq!(percents(&pool, user_id, 0).await, Some(vec![]));
        assert_eq!(percents(&pool, user_id, -1).await, None);
    }

    /// 購入の枠を買った順に並べ、無料の枠を最後に置く。使い切った購入の枠は並べない。
    #[sqlx::test]
    async fn quotas_list_purchases_in_order_and_the_free_quota_last(pool: SqlitePool) {
        let user_id = insert_user(&pool, "kyoko").await;
        insert_ocr_quota_grant(&pool, user_id, "cs_first", TEST_TOPUP_GRANT_MILLI_YEN / 2).await;
        insert_ocr_quota_grant(&pool, user_id, "cs_used_up", 0).await;
        insert_ocr_quota_grant(&pool, user_id, "cs_second", TEST_TOPUP_GRANT_MILLI_YEN).await;
        assert_eq!(percents(&pool, user_id, 50).await, Some(vec![50, 100, 100]));
        // 無料の枠が無制限なら、購入の枠も見ない。
        assert_eq!(percents(&pool, user_id, -1).await, None);
    }

    /// 同じユーザーの読み取りは1件ずつで、終わればまた始められる。ほかのユーザーは妨げない。
    #[test]
    fn in_flight_allows_one_reading_per_user() {
        let in_flight = Arc::new(InFlight::default());
        let first = in_flight.begin(1).expect("first reading should start");
        assert!(matches!(in_flight.begin(1), Err(AppError::OcrInProgress)));
        let _other = in_flight
            .begin(2)
            .expect("another user should not be blocked");
        drop(first);
        in_flight
            .begin(1)
            .expect("should start again after the first one ends");
    }

    /// 使うたびの記録を、残した順に (種類, 購入の行, 国内か, 使った量, 売値に直した額)。
    async fn usage_records(pool: &SqlitePool) -> Vec<(String, Option<i64>, Option<i64>, i64, i64)> {
        sqlx::query!(
            "SELECT kind, grant_id, domestic, used_milli_yen, sales_milli_yen FROM ocr_usage_records ORDER BY id"
        )
        .fetch_all(pool)
        .await
        .expect("failed to read usage records")
        .into_iter()
        .map(|row| {
            (
                row.kind,
                row.grant_id,
                row.domestic,
                row.used_milli_yen,
                row.sales_milli_yen,
            )
        })
        .collect()
    }

    async fn grant_id(pool: &SqlitePool, checkout_session_id: &str) -> i64 {
        sqlx::query_scalar!(
            r#"SELECT id AS "id!: i64" FROM ocr_quota_grants WHERE stripe_checkout_session_id = ?"#,
            checkout_session_id
        )
        .fetch_one(pool)
        .await
        .expect("grant should exist")
    }

    /// 買った枠から使った分は、購入ごとに売値に直した額と国内かを残し、無料の枠の分は額 0 で残す
    /// (docs/payments.md)。
    #[sqlx::test]
    async fn each_use_is_recorded_per_grant_with_its_sales_amount(pool: SqlitePool) {
        let user_id = insert_user(&pool, "kyoko").await;
        insert_ocr_quota_grant(&pool, user_id, "cs_domestic", 1_000).await;
        insert_ocr_quota_grant(&pool, user_id, "cs_abroad", TEST_TOPUP_GRANT_MILLI_YEN).await;
        sqlx::query!(
            "UPDATE ocr_quota_grants SET domestic = (stripe_checkout_session_id = 'cs_domestic')"
        )
        .execute(&pool)
        .await
        .expect("failed to set domestic");
        let domestic = grant_id(&pool, "cs_domestic").await;
        let abroad = grant_id(&pool, "cs_abroad").await;

        // 1つ目の購入の残り (1円) を使い切り、2つ目から 0.5円。300円で 60円分なので 5倍。
        add_spent(&pool, user_id, 1_500, 50)
            .await
            .expect("add_spent");
        // 買った枠を使い切った後の分は、無料の枠から。
        add_spent(&pool, user_id, TEST_TOPUP_GRANT_MILLI_YEN - 500 + 2_000, 50)
            .await
            .expect("add_spent");

        assert_eq!(
            usage_records(&pool).await,
            vec![
                ("paid".to_string(), Some(domestic), Some(1), 1_000, 5_000),
                ("paid".to_string(), Some(abroad), Some(0), 500, 2_500),
                ("paid".to_string(), Some(abroad), Some(0), 59_500, 297_500),
                ("free".to_string(), None, None, 2_000, 0),
            ]
        );
        assert_eq!(spent(&pool, user_id).await, 2_000);
    }

    /// Apple の Sandbox (審査・TestFlight) の枠は売上ではないので、額を付けずに残す。
    #[sqlx::test]
    async fn uses_of_sandbox_grants_are_not_sales(pool: SqlitePool) {
        let user_id = insert_user(&pool, "kyoko").await;
        sqlx::query!(
            "INSERT INTO ocr_quota_grants (user_id, granted_milli_yen, remaining_milli_yen, purchased_at, price_yen, apple_transaction_id, apple_environment, apple_signed_date, domestic) VALUES (?, ?, ?, '2026-01-01T00:00:00.000Z', 300, '2000000001', 'Sandbox', 1, 1)",
            user_id,
            TEST_TOPUP_GRANT_MILLI_YEN,
            TEST_TOPUP_GRANT_MILLI_YEN
        )
        .execute(&pool)
        .await
        .expect("failed to insert a sandbox grant");

        add_spent(&pool, user_id, 1_000, 50)
            .await
            .expect("add_spent");

        let records = usage_records(&pool).await;
        assert_eq!(records.len(), 1);
        assert_eq!(
            (records[0].0.as_str(), records[0].2, records[0].4),
            ("sandbox", None, 0)
        );
    }

    /// 無料の枠が無制限でも、使った分は無料の分として残す。
    #[sqlx::test]
    async fn uses_under_an_unlimited_budget_are_recorded_as_free(pool: SqlitePool) {
        let user_id = insert_user(&pool, "kyoko").await;
        insert_ocr_quota_grant(&pool, user_id, "cs_1", TEST_TOPUP_GRANT_MILLI_YEN).await;

        add_spent(&pool, user_id, 1_000, -1)
            .await
            .expect("add_spent");

        assert_eq!(
            usage_records(&pool).await,
            vec![("free".to_string(), None, None, 1_000, 0)]
        );
        assert_eq!(
            paid_remaining(&pool, user_id).await,
            TEST_TOPUP_GRANT_MILLI_YEN
        );
    }
}
