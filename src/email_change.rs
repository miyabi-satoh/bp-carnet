//! メールアドレスの変更 (docs/authentication.md)。
//!
//! ユーザーID (`users.username`) がメールアドレスを兼ねるので、変えるのはユーザーID。新しいアドレスに
//! 届いたリンクで確かめてから変え、古いアドレスにも知らせる。

use sqlx::{Connection, SqlitePool};

use crate::error::AppError;

/// `user_id` のメールアドレスを `new_email` (正規化済み) に変え、変える前のユーザーIDを返す。
/// 対象のアカウントが無い・凍結中なら `None`。
///
/// 新しいアドレスに未確認のアカウント (サインアップの確認待ち) があれば消してから変える
/// (管理者がユーザーを作るときと同じ。`crate::auth::discard_unverified_user`)。確認済みの
/// アカウントが使っていれば `AppError::UsernameTaken`。
pub async fn complete(
    pool: &SqlitePool,
    user_id: i64,
    new_email: &str,
) -> Result<Option<String>, AppError> {
    let mut conn = pool.acquire().await?;
    // 未確認のアカウントの削除と変更の間に、同じアドレスの申し込みが割り込まないよう、最初から書き込みの
    // ロックを取る (`crate::signup::register` と同じ理由)。
    let mut tx = conn.begin_with("BEGIN IMMEDIATE").await?;
    // 凍結中・消えたアカウントは変えない (凍結は本人の操作も止めるため)。未確認のアカウントを消すより先に確かめる。
    let Some(old) = sqlx::query_scalar!(
        "SELECT username FROM users WHERE id = ? AND frozen = 0",
        user_id
    )
    .fetch_optional(&mut *tx)
    .await?
    else {
        return Ok(None);
    };
    sqlx::query!(
        "DELETE FROM users WHERE username = ? AND email_verified = 0 AND id != ?",
        new_email,
        user_id
    )
    .execute(&mut *tx)
    .await?;
    let updated = sqlx::query!(
        "UPDATE users SET username = ?, email_verified = 1 WHERE id = ?",
        new_email,
        user_id
    )
    .execute(&mut *tx)
    .await;
    match updated {
        Ok(_) => {}
        Err(sqlx::Error::Database(err)) if err.is_unique_violation() => {
            return Err(AppError::UsernameTaken);
        }
        Err(err) => return Err(err.into()),
    }
    // 古いアドレスに送ったリンク (再設定など) を使えなくする。古いメールボックスを失った・他人に
    // 渡ったことが変える理由になりうるため。
    sqlx::query!("DELETE FROM email_tokens WHERE user_id = ?", user_id)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(Some(old))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::insert_user;

    #[sqlx::test]
    async fn changes_the_username_and_discards_an_unverified_account(pool: SqlitePool) {
        let user_id = insert_user(&pool, "old@example.com").await;
        let pending = insert_user(&pool, "new@example.com").await;
        sqlx::query!("UPDATE users SET email_verified = 0 WHERE id = ?", pending)
            .execute(&pool)
            .await
            .expect("未確認にできるはず");

        let old = complete(&pool, user_id, "new@example.com")
            .await
            .expect("変えられるはず");

        assert_eq!(old.as_deref(), Some("old@example.com"));
        let row = sqlx::query!(
            r#"SELECT username, email_verified AS "email_verified!: i64" FROM users WHERE id = ?"#,
            user_id
        )
        .fetch_one(&pool)
        .await
        .expect("読めるはず");
        assert_eq!(
            (row.username.as_str(), row.email_verified),
            ("new@example.com", 1)
        );
    }

    #[sqlx::test]
    async fn links_sent_to_the_old_address_stop_working(pool: SqlitePool) {
        let user_id = insert_user(&pool, "old@example.com").await;
        let reset =
            crate::email_token::issue(&pool, user_id, crate::email_token::Purpose::ResetPassword)
                .await
                .expect("発行できるはず")
                .expect("発行されるはず");

        complete(&pool, user_id, "new@example.com")
            .await
            .expect("変えられるはず");

        let usable = crate::email_token::is_usable(
            &pool,
            &reset,
            crate::email_token::Purpose::ResetPassword,
        )
        .await
        .expect("確かめられるはず");
        assert!(!usable);
    }

    #[sqlx::test]
    async fn frozen_accounts_are_left_unchanged(pool: SqlitePool) {
        let user_id = insert_user(&pool, "old@example.com").await;
        sqlx::query!("UPDATE users SET frozen = 1 WHERE id = ?", user_id)
            .execute(&pool)
            .await
            .expect("凍結できるはず");

        let old = complete(&pool, user_id, "new@example.com")
            .await
            .expect("読めるはず");

        assert!(old.is_none());
    }

    #[sqlx::test]
    async fn refuses_an_address_of_a_verified_account(pool: SqlitePool) {
        let user_id = insert_user(&pool, "old@example.com").await;
        insert_user(&pool, "taken@example.com").await;

        let result = complete(&pool, user_id, "taken@example.com").await;

        assert!(matches!(result, Err(AppError::UsernameTaken)));
    }
}
