//! メールでのパスワードの再設定 (docs/authentication.md)。

use sqlx::SqlitePool;

use crate::email_token::{self, Purpose};

/// `username` (正規化済みのメールアドレス) の、メール確認済みのアカウントに再設定のトークンを
/// 発行する。該当するアカウントが無い、または直前 ([`email_token::issue`] の間隔以内) に発行した
/// リンクがまだ使われていなければ `None` (メールを送らない)。
///
/// 未確認のアカウント (サインアップの確認待ち) は、無いものとして扱う。パスワードを持つ前の
/// アカウントなので、再設定ではなくサインアップの確認リンクで設定するため。
pub async fn issue(pool: &SqlitePool, username: &str) -> Result<Option<String>, sqlx::Error> {
    let Some(user_id) = crate::auth::find_verified_user_id(pool, username).await? else {
        return Ok(None);
    };
    email_token::issue(pool, user_id, Purpose::ResetPassword).await
}

/// パスワードを再設定する。再設定できたら `true`、対象が未確認のアカウントなら `false`。
///
/// 確認リンクのトークンは、呼び出し側が先に [`email_token::consume`] で使い済みにしておくこと。
///
/// パスワードを持たない Google 専用アカウントにも持たせる (`password_usable = 1`)。メールを
/// 受け取れたことが本人の確認になるため。対象のセッションはすべて断つ: パスワードを忘れた
/// ときだけでなく、乗っ取りを疑ったときにも使われるため。
pub async fn complete(
    pool: &SqlitePool,
    user_id: i64,
    password_hash: &str,
) -> Result<bool, sqlx::Error> {
    let updated = sqlx::query!(
        "UPDATE users
         SET password_hash = ?, password_usable = 1,
             session_generation = session_generation + 1
         WHERE id = ? AND email_verified = 1",
        password_hash,
        user_id
    )
    .execute(pool)
    .await?;
    Ok(updated.rows_affected() > 0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::auth::{self, NewPassword, Role};

    #[sqlx::test]
    async fn issue_returns_none_for_unknown_and_unverified_accounts(pool: SqlitePool) {
        crate::signup::register(&pool, "pending@example.com", "unusable", "1")
            .await
            .expect("受け付けられるはず");

        let unknown = issue(&pool, "nobody@example.com")
            .await
            .expect("読めるはず");
        let unverified = issue(&pool, "pending@example.com")
            .await
            .expect("読めるはず");

        assert!(unknown.is_none());
        assert!(unverified.is_none());
    }

    #[sqlx::test]
    async fn complete_gives_a_password_and_cuts_every_session(pool: SqlitePool) {
        let user_id = auth::create_user(
            &pool,
            "google@example.com",
            NewPassword::Unusable("unusable"),
            Role::User,
        )
        .await
        .expect("テストユーザーを作成できるはず");
        let token = issue(&pool, "google@example.com")
            .await
            .expect("発行できるはず")
            .expect("確認済みのアカウントには発行されるはず");
        email_token::consume(&pool, &token, Purpose::ResetPassword)
            .await
            .expect("照合できるはず")
            .expect("使えるはず");

        let done = complete(&pool, user_id, "new-hash")
            .await
            .expect("更新できるはず");

        assert!(done);
        let row = sqlx::query!(
            r#"SELECT password_hash, password_usable, session_generation AS "session_generation!: i64"
               FROM users WHERE id = ?"#,
            user_id
        )
        .fetch_one(&pool)
        .await
        .expect("読めるはず");
        assert_eq!(
            (
                row.password_hash.as_str(),
                row.password_usable,
                row.session_generation
            ),
            ("new-hash", 1, 1)
        );
    }
}
