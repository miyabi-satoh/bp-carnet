//! オープンサインアップ (docs/authentication.md)。
//!
//! サインアップで作るのは、パスワードを持たない未確認のアカウント (`email_verified = 0`・
//! `password_usable = 0`) だけ。パスワードは確認リンクの先で設定し、その時点で確認済みにする。
//! 未確認のアカウントは誰もログインできず記録も持たないため、他人に先に登録されても、
//! 本人の登録し直しや Google ログインでそのまま引き継げる。

use sqlx::SqlitePool;

use crate::auth::Role;
use crate::email_token::{self, Purpose};

/// サインアップの受け付け結果。
#[derive(Debug)]
pub enum Registration {
    /// 未確認のアカウント (新規・登録し直し)。確認メールに載せるトークンを持つ。
    Unverified { token: String },
    /// 未確認のアカウントで、使われていない確認リンクを直前に発行済み
    /// ([`email_token::issue`] の間隔以内)。発行し直さない。
    LinkRecentlyIssued,
    /// 既に確認済みのアカウントがある。
    AlreadyRegistered,
}

/// `username` (正規化済みのメールアドレス) のサインアップを受け付ける。
///
/// 未確認のアカウントが無ければ作り、あれば確認リンクを発行し直す (前に送ったリンクは
/// 使えなくなる。ただし使われていないリンクを直前に発行済みなら発行し直さない)。
/// `unusable_hash` は新規作成時の `password_hash` (NOT NULL のため空にできない)。
/// `terms_version` は同意した規約の版。
///
/// ADR: 作成と既存の読み取りを1文の upsert にし、トランザクションの最初の文を書き込みにする。
/// 読みを先に置くと、定期処理による未確認アカウントの削除と挟まってトークンの発行が外部キー
/// 違反になりうるほか、書き込みへの昇格で `SQLITE_BUSY_SNAPSHOT` になりうる。
pub async fn register(
    pool: &SqlitePool,
    username: &str,
    unusable_hash: &str,
    terms_version: &str,
) -> Result<Registration, sqlx::Error> {
    let mut tx = pool.begin().await?;
    // 同意の記録は、未確認のアカウントでだけ新しくする (確認済みのアカウントは、この申し込みでは何も変えない)。
    let user = sqlx::query!(
        r#"INSERT INTO users (username, password_hash, password_usable, email_verified, role,
                              terms_version, terms_agreed_at)
           VALUES (?1, ?2, 0, 0, ?3, ?4, strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
           ON CONFLICT (username) DO UPDATE SET
               username = excluded.username,
               terms_version = CASE WHEN users.email_verified = 0
                                    THEN excluded.terms_version ELSE users.terms_version END,
               terms_agreed_at = CASE WHEN users.email_verified = 0
                                      THEN excluded.terms_agreed_at ELSE users.terms_agreed_at END
           RETURNING id AS "id!: i64", email_verified"#,
        username,
        unusable_hash,
        Role::User,
        terms_version
    )
    .fetch_one(&mut *tx)
    .await?;

    if user.email_verified != 0 {
        tx.commit().await?;
        return Ok(Registration::AlreadyRegistered);
    }

    let token = email_token::issue(&mut *tx, user.id, Purpose::VerifyEmail).await?;
    tx.commit().await?;
    Ok(match token {
        Some(token) => Registration::Unverified { token },
        None => Registration::LinkRecentlyIssued,
    })
}

/// `username` の未確認のアカウントに、確認リンクを発行し直す (パスワードの再設定の申し込みから)。
/// 未確認のアカウントが無い、または使われていない確認リンクを直前に発行済みなら `None`。
///
/// 同意の記録は変えない (サインアップの申し込みで記録済み)。
///
/// ADR: [`register`] と同じく、トランザクションの最初の文を書き込みにする。読みを先に置くと、
/// 定期処理による未確認アカウントの削除と挟まってトークンの発行が外部キー違反になりうるため。
pub async fn reissue_verification(
    pool: &SqlitePool,
    username: &str,
) -> Result<Option<String>, sqlx::Error> {
    let mut tx = pool.begin().await?;
    let user_id = sqlx::query_scalar!(
        r#"UPDATE users SET username = username WHERE username = ? AND email_verified = 0
           RETURNING id AS "id!: i64""#,
        username
    )
    .fetch_optional(&mut *tx)
    .await?;
    let Some(user_id) = user_id else {
        return Ok(None);
    };
    let token = email_token::issue(&mut *tx, user_id, Purpose::VerifyEmail).await?;
    tx.commit().await?;
    Ok(token)
}

/// パスワードを設定して確認済みにする。確認済みにできたら `true`、対象が既に確認済み
/// (その間に Google ログインで引き継がれた) なら `false`。
///
/// 確認リンクのトークンは、呼び出し側が先に [`email_token::consume`] で使い済みにしておくこと。
///
/// 確認済みのアカウントには設定しない。Google ログインで引き継がれた後に古いリンクが使われると、
/// 本人が作っていないパスワードでログインできる経路ができるため。
pub async fn complete(
    pool: &SqlitePool,
    user_id: i64,
    password_hash: &str,
) -> Result<bool, sqlx::Error> {
    let updated = sqlx::query!(
        "UPDATE users
         SET password_hash = ?, password_usable = 1, email_verified = 1,
             session_generation = session_generation + 1
         WHERE id = ? AND email_verified = 0",
        password_hash,
        user_id
    )
    .execute(pool)
    .await?;
    Ok(updated.rows_affected() > 0)
}

/// 確認リンクの期限が切れた未確認のアカウントを削除する。削除した件数を返す。
pub async fn purge_expired_unverified(pool: &SqlitePool) -> Result<u64, sqlx::Error> {
    let purpose = Purpose::VerifyEmail.as_str();
    let result = sqlx::query!(
        r#"
        DELETE FROM users
        WHERE email_verified = 0
          AND NOT EXISTS (
              SELECT 1 FROM email_tokens
              WHERE email_tokens.user_id = users.id AND email_tokens.purpose = ?
                AND email_tokens.expires_at > strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
          )
        "#,
        purpose
    )
    .execute(pool)
    .await?;
    Ok(result.rows_affected())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::insert_user;

    async fn register_token(pool: &SqlitePool, username: &str) -> String {
        match register(pool, username, "unusable-hash", "1")
            .await
            .expect("受け付けられるはず")
        {
            Registration::Unverified { token } => token,
            other => panic!("未確認として発行されるはず: {other:?}"),
        }
    }

    async fn consume_token(pool: &SqlitePool, token: &str) -> Option<i64> {
        email_token::consume(pool, token, Purpose::VerifyEmail)
            .await
            .expect("照合できるはず")
    }

    async fn user_count(pool: &SqlitePool) -> i64 {
        sqlx::query_scalar!(r#"SELECT COUNT(*) AS "count!: i64" FROM users"#)
            .fetch_one(pool)
            .await
            .expect("数えられるはず")
    }

    /// 直後の登録し直しでは発行し直さず、前のリンクがそのまま使える。
    #[sqlx::test]
    async fn register_again_soon_keeps_the_previous_link(pool: SqlitePool) {
        let old = register_token(&pool, "kyoko@example.com").await;

        let again = register(&pool, "kyoko@example.com", "unusable-hash", "1")
            .await
            .expect("受け付けられるはず");

        assert!(matches!(again, Registration::LinkRecentlyIssued));
        assert_eq!(user_count(&pool).await, 1);
        assert!(consume_token(&pool, &old).await.is_some());
    }

    /// 同時に申し込まれても、発行するのは片方だけ。
    #[sqlx::test]
    async fn concurrent_registrations_issue_only_one_link(pool: SqlitePool) {
        let (first, second) = tokio::join!(
            register(&pool, "kyoko@example.com", "unusable-hash", "1"),
            register(&pool, "kyoko@example.com", "unusable-hash", "1"),
        );
        let results = [
            first.expect("受け付けられるはず"),
            second.expect("受け付けられるはず"),
        ];

        let tokens: Vec<_> = results
            .iter()
            .filter_map(|result| match result {
                Registration::Unverified { token } => Some(token),
                _ => None,
            })
            .collect();
        assert_eq!(tokens.len(), 1, "{results:?}");
        let skipped = results
            .iter()
            .filter(|result| matches!(result, Registration::LinkRecentlyIssued))
            .count();
        assert_eq!(skipped, 1, "{results:?}");
        assert_eq!(user_count(&pool).await, 1);
        assert!(consume_token(&pool, tokens[0]).await.is_some());
    }

    /// 間隔を空けた登録し直しでは、同じアカウントのまま発行し直し、前のリンクは使えなくなる。
    #[sqlx::test]
    async fn register_again_later_reuses_the_account_and_invalidates_the_previous_link(
        pool: SqlitePool,
    ) {
        let old = register_token(&pool, "kyoko@example.com").await;
        email_token::backdate_issue(&pool, Purpose::VerifyEmail, 10).await;

        let new = register_token(&pool, "kyoko@example.com").await;

        assert_eq!(user_count(&pool).await, 1);
        assert_eq!(consume_token(&pool, &old).await, None);
        assert!(consume_token(&pool, &new).await.is_some());
    }

    /// 未確認のアカウントにだけ発行し直す。直後の申し込み直しでは発行し直さない。
    #[sqlx::test]
    async fn reissue_verification_only_for_unverified_accounts(pool: SqlitePool) {
        let old = register_token(&pool, "kyoko@example.com").await;
        insert_user(&pool, "saburo@example.com").await;

        let soon = reissue_verification(&pool, "kyoko@example.com")
            .await
            .expect("読めるはず");
        email_token::backdate_issue(&pool, Purpose::VerifyEmail, 10).await;
        let later = reissue_verification(&pool, "kyoko@example.com")
            .await
            .expect("読めるはず");
        let verified = reissue_verification(&pool, "saburo@example.com")
            .await
            .expect("読めるはず");
        let unknown = reissue_verification(&pool, "nobody@example.com")
            .await
            .expect("読めるはず");

        assert_eq!(soon, None);
        let later = later.expect("間隔を空ければ発行し直すはず");
        assert_eq!(consume_token(&pool, &old).await, None);
        assert!(consume_token(&pool, &later).await.is_some());
        assert_eq!(verified, None);
        assert_eq!(unknown, None);
        assert_eq!(user_count(&pool).await, 2);
    }

    async fn terms_agreement(
        pool: &SqlitePool,
        username: &str,
    ) -> (Option<String>, Option<String>) {
        let row = sqlx::query!(
            "SELECT terms_version, terms_agreed_at FROM users WHERE username = ?",
            username
        )
        .fetch_one(pool)
        .await
        .expect("読めるはず");
        (row.terms_version, row.terms_agreed_at)
    }

    /// 未確認のアカウントに同意した版と日時を残す。
    #[sqlx::test]
    async fn register_records_the_terms_agreement(pool: SqlitePool) {
        register(&pool, "kyoko@example.com", "unusable-hash", "1")
            .await
            .expect("受け付けられるはず");

        let (version, agreed_at) = terms_agreement(&pool, "kyoko@example.com").await;
        assert_eq!(version.as_deref(), Some("1"));
        assert!(agreed_at.is_some());
    }

    /// 登録し直しでは未確認のアカウントの同意を新しい版に改め、確認済みのアカウントは変えない。
    #[sqlx::test]
    async fn register_again_updates_the_agreement_only_while_unverified(pool: SqlitePool) {
        register(&pool, "kyoko@example.com", "unusable-hash", "1")
            .await
            .expect("受け付けられるはず");
        register(&pool, "kyoko@example.com", "unusable-hash", "2")
            .await
            .expect("受け付けられるはず");
        insert_user(&pool, "saburo@example.com").await;
        register(&pool, "saburo@example.com", "unusable-hash", "2")
            .await
            .expect("受け付けられるはず");

        assert_eq!(
            terms_agreement(&pool, "kyoko@example.com")
                .await
                .0
                .as_deref(),
            Some("2")
        );
        assert_eq!(
            terms_agreement(&pool, "saburo@example.com").await,
            (None, None)
        );
    }

    #[sqlx::test]
    async fn register_reports_an_existing_verified_account(pool: SqlitePool) {
        insert_user(&pool, "kyoko@example.com").await;

        let registration = register(&pool, "kyoko@example.com", "unusable-hash", "1")
            .await
            .expect("受け付けられるはず");

        assert!(matches!(registration, Registration::AlreadyRegistered));
        let hash = sqlx::query_scalar!(
            "SELECT password_hash FROM users WHERE username = 'kyoko@example.com'"
        )
        .fetch_one(&pool)
        .await
        .expect("読めるはず");
        assert_eq!(hash, "dummy-hash", "既存のパスワードを書き換えてはいけない");
    }

    #[sqlx::test]
    async fn complete_sets_the_password_and_verifies_the_email(pool: SqlitePool) {
        let token = register_token(&pool, "kyoko@example.com").await;

        let user_id = consume_token(&pool, &token)
            .await
            .expect("トークンを使えるはず");

        let verified = complete(&pool, user_id, "new-hash")
            .await
            .expect("更新できるはず");

        assert!(verified);

        let row = sqlx::query!(
            "SELECT password_hash, password_usable, email_verified FROM users WHERE id = ?",
            user_id
        )
        .fetch_one(&pool)
        .await
        .expect("読めるはず");
        assert_eq!(
            (
                row.password_hash.as_str(),
                row.password_usable,
                row.email_verified
            ),
            ("new-hash", 1, 1)
        );
    }

    #[sqlx::test]
    async fn complete_does_not_touch_an_account_verified_in_the_meantime(pool: SqlitePool) {
        let token = register_token(&pool, "kyoko@example.com").await;
        let user_id = consume_token(&pool, &token)
            .await
            .expect("トークンを使えるはず");
        sqlx::query!("UPDATE users SET email_verified = 1")
            .execute(&pool)
            .await
            .expect("テストデータを更新できるはず");

        let verified = complete(&pool, user_id, "new-hash")
            .await
            .expect("更新できるはず");

        assert!(!verified);
        let hash = sqlx::query_scalar!("SELECT password_hash FROM users WHERE id = ?", user_id)
            .fetch_one(&pool)
            .await
            .expect("読めるはず");
        assert_eq!(hash, "unusable-hash");
    }

    #[sqlx::test]
    async fn purge_removes_only_unverified_accounts_whose_link_expired(pool: SqlitePool) {
        register_token(&pool, "expired@example.com").await;
        register_token(&pool, "pending@example.com").await;
        insert_user(&pool, "verified@example.com").await;
        sqlx::query!(
            "UPDATE email_tokens SET expires_at = '2000-01-01T00:00:00.000Z'
             WHERE user_id = (SELECT id FROM users WHERE username = 'expired@example.com')"
        )
        .execute(&pool)
        .await
        .expect("テストデータを更新できるはず");

        let deleted = purge_expired_unverified(&pool)
            .await
            .expect("削除できるはず");

        assert_eq!(deleted, 1);
        let remaining = sqlx::query_scalar!("SELECT username FROM users ORDER BY username")
            .fetch_all(&pool)
            .await
            .expect("読めるはず");
        assert_eq!(
            remaining,
            vec!["pending@example.com", "verified@example.com"]
        );
    }
}
