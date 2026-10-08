//! メール内リンクに載せる1回限りのトークン (`email_tokens`、docs/authentication.md)。
//!
//! トークン本体は利用者に送るだけで保存せず、DB にはハッシュだけを持つ。

use sqlx::SqliteExecutor;

use crate::token::{random_url_safe, sha256_url_safe};

/// トークンの用途。用途ごとに有効期限が違い、別の用途のリンクとしては使えない。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Purpose {
    VerifyEmail,
    ResetPassword,
    ChangeEmail,
}

impl Purpose {
    /// `email_tokens.purpose` に保存する値。
    pub fn as_str(self) -> &'static str {
        match self {
            Self::VerifyEmail => "verify_email",
            Self::ResetPassword => "reset_password",
            Self::ChangeEmail => "change_email",
        }
    }

    /// 発行からの有効期限 (時間)。
    ///
    /// ADR: リセットとメールアドレスの変更は乗っ取りに直結するため短く、確認はすぐメールを
    /// 開かない人もいるため長めにする (docs/authentication.md)。
    fn lifetime_hours(self) -> u32 {
        match self {
            Self::VerifyEmail => 24,
            Self::ResetPassword | Self::ChangeEmail => 1,
        }
    }

    /// 発行日時から `expires_at` へ進める、SQLite の日時修飾子。
    fn lifetime_modifier(self) -> String {
        format!("+{} hours", self.lifetime_hours())
    }

    /// `expires_at` から発行日時へ戻す、SQLite の日時修飾子 ([`Self::lifetime_modifier`] の逆)。
    fn issued_at_modifier(self) -> String {
        format!("-{} hours", self.lifetime_hours())
    }
}

/// ADR: 32バイト (base64url で43文字)。推測でリンクを当てられない長さにする (OAuth の state と同じ)。
const TOKEN_BYTES: usize = 32;

/// ADR: 使われていないリンクを、申し込み直されても発行し直さない間隔 (サインアップの確認・
/// 再設定で共通)。短いと第三者の申し込み直しで本人に届いたリンクを潰し続けられ、長いと
/// 届かなかった人が申し込み直せるまで待たされる。本人がメールを開くまでの時間として5分にする。
const REISSUE_COOLDOWN: &str = "-5 minutes";

/// トークンを発行し、メールに載せる本体を返す。まだ使われていないトークンを
/// `REISSUE_COOLDOWN` 以内に発行済みなら、発行し直さずに `None` を返す。
///
/// 同じユーザー・用途のトークンは1つだけで、発行し直すと上書きする。前に送ったリンクが後から
/// 使われないようにするため。間隔を置くのは、第三者が同じ相手の発行を繰り返し、本人に届いた
/// リンクを使えなくし続けるのを防ぐため。判定と上書きは1文で行うので、同時に発行されても
/// 両方が発行し直すことは無い。
pub async fn issue<'e, E: SqliteExecutor<'e>>(
    executor: E,
    user_id: i64,
    purpose: Purpose,
) -> Result<Option<String>, sqlx::Error> {
    issue_with(executor, user_id, purpose, None).await
}

/// メールアドレスの変更のトークンを、送り先の新しいアドレスと一緒に発行する。間隔の扱いは
/// [`issue`] と同じだが、前と違うアドレスへの申し込みなら間隔の内でも発行し直す (打ち間違いを
/// 直してすぐ申し込み直せるように)。
pub async fn issue_change_email<'e, E: SqliteExecutor<'e>>(
    executor: E,
    user_id: i64,
    new_email: &str,
) -> Result<Option<String>, sqlx::Error> {
    issue_with(executor, user_id, Purpose::ChangeEmail, Some(new_email)).await
}

async fn issue_with<'e, E: SqliteExecutor<'e>>(
    executor: E,
    user_id: i64,
    purpose: Purpose,
    new_email: Option<&str>,
) -> Result<Option<String>, sqlx::Error> {
    let token = random_url_safe(TOKEN_BYTES);
    let purpose_str = purpose.as_str();
    let token_hash = sha256_url_safe(&token);
    let lifetime = purpose.lifetime_modifier();
    let issued_at = purpose.issued_at_modifier();
    let result = sqlx::query!(
        r#"INSERT INTO email_tokens (user_id, purpose, token_hash, expires_at, new_email)
           VALUES (?, ?, ?, strftime('%Y-%m-%dT%H:%M:%fZ', 'now', ?), ?)
           ON CONFLICT (user_id, purpose) DO UPDATE
           SET token_hash = excluded.token_hash, expires_at = excluded.expires_at, used_at = NULL,
               new_email = excluded.new_email
           WHERE email_tokens.used_at IS NOT NULL
              OR email_tokens.new_email IS NOT excluded.new_email
              OR strftime('%Y-%m-%dT%H:%M:%fZ', email_tokens.expires_at, ?)
                 <= strftime('%Y-%m-%dT%H:%M:%fZ', 'now', ?)"#,
        user_id,
        purpose_str,
        token_hash,
        lifetime,
        new_email,
        issued_at,
        REISSUE_COOLDOWN
    )
    .execute(executor)
    .await?;

    Ok((result.rows_affected() > 0).then_some(token))
}

/// トークンを使用済みにし、紐付くユーザーの id を返す。未知・用途違い・期限切れ・使用済みなら
/// `None`。
///
/// 判定と使用済みへの更新を1文で行う。別々にすると、同じリンクを同時に2回開いたときに両方が
/// 通りうるため。
pub async fn consume<'e, E: SqliteExecutor<'e>>(
    executor: E,
    token: &str,
    purpose: Purpose,
) -> Result<Option<i64>, sqlx::Error> {
    let token_hash = sha256_url_safe(token);
    let purpose_str = purpose.as_str();
    sqlx::query_scalar!(
        r#"UPDATE email_tokens SET used_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
           WHERE token_hash = ? AND purpose = ? AND used_at IS NULL
             AND expires_at > strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
           RETURNING user_id AS "user_id!: i64""#,
        token_hash,
        purpose_str
    )
    .fetch_optional(executor)
    .await
}

/// メールアドレスの変更のトークンを使用済みにし、紐付くユーザーの id と新しいアドレスを返す。
/// 使えなければ `None` ([`consume`] と同じ)。
pub async fn consume_change_email<'e, E: SqliteExecutor<'e>>(
    executor: E,
    token: &str,
) -> Result<Option<(i64, String)>, sqlx::Error> {
    let token_hash = sha256_url_safe(token);
    let purpose_str = Purpose::ChangeEmail.as_str();
    let row = sqlx::query!(
        r#"UPDATE email_tokens SET used_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
           WHERE token_hash = ? AND purpose = ? AND used_at IS NULL
             AND expires_at > strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
           RETURNING user_id AS "user_id!: i64", new_email AS "new_email!: String""#,
        token_hash,
        purpose_str
    )
    .fetch_optional(executor)
    .await?;
    Ok(row.map(|row| (row.user_id, row.new_email)))
}

/// メールアドレスの変更のトークンがまだ使えれば、新しいアドレスを返す (使用済みにはしない)。
/// リンクを開いたときに、変更先を見せて確かめてもらうため。
pub async fn change_email_target<'e, E: SqliteExecutor<'e>>(
    executor: E,
    token: &str,
) -> Result<Option<String>, sqlx::Error> {
    let token_hash = sha256_url_safe(token);
    let purpose_str = Purpose::ChangeEmail.as_str();
    sqlx::query_scalar!(
        r#"SELECT new_email AS "new_email!: String" FROM email_tokens
           WHERE token_hash = ? AND purpose = ? AND used_at IS NULL
             AND expires_at > strftime('%Y-%m-%dT%H:%M:%fZ', 'now')"#,
        token_hash,
        purpose_str
    )
    .fetch_optional(executor)
    .await
}

/// トークンがまだ使えるか (未知・用途違い・期限切れ・使用済みでないか) を、使用済みにせずに確かめる。
/// リンクを開いた時点で使えないことを知らせるため。使えるかどうかの最終的な判定は [`consume`] で行う。
pub async fn is_usable<'e, E: SqliteExecutor<'e>>(
    executor: E,
    token: &str,
    purpose: Purpose,
) -> Result<bool, sqlx::Error> {
    let token_hash = sha256_url_safe(token);
    let purpose_str = purpose.as_str();
    sqlx::query_scalar!(
        r#"SELECT EXISTS (
               SELECT 1 FROM email_tokens
               WHERE token_hash = ? AND purpose = ? AND used_at IS NULL
                 AND expires_at > strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
           ) AS "usable!: bool""#,
        token_hash,
        purpose_str
    )
    .fetch_one(executor)
    .await
}

/// テスト用: `purpose` のトークンをすべて、`minutes` 分前に発行したことにする。
#[cfg(test)]
pub(crate) async fn backdate_issue<'e, E: SqliteExecutor<'e>>(
    executor: E,
    purpose: Purpose,
    minutes: u32,
) {
    let issued_at = format!("-{minutes} minutes");
    let lifetime = purpose.lifetime_modifier();
    let purpose_str = purpose.as_str();
    sqlx::query!(
        "UPDATE email_tokens SET expires_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now', ?, ?)
         WHERE purpose = ?",
        issued_at,
        lifetime,
        purpose_str
    )
    .execute(executor)
    .await
    .expect("テストデータを更新できるはず");
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{conn, insert_user};
    use sqlx::SqlitePool;

    #[sqlx::test]
    async fn consume_accepts_a_token_only_once(pool: SqlitePool) {
        let user_id = insert_user(&pool, "kyoko").await;
        let mut conn = conn(&pool).await;
        let token = issue(&mut *conn, user_id, Purpose::ResetPassword)
            .await
            .expect("発行できるはず")
            .expect("初回は発行されるはず");

        let first = consume(&mut *conn, &token, Purpose::ResetPassword)
            .await
            .expect("照合できるはず");
        let second = consume(&mut *conn, &token, Purpose::ResetPassword)
            .await
            .expect("照合できるはず");

        assert_eq!(first, Some(user_id));
        assert_eq!(second, None);
    }

    #[sqlx::test]
    async fn consume_rejects_a_token_for_another_purpose(pool: SqlitePool) {
        let user_id = insert_user(&pool, "kyoko").await;
        let mut conn = conn(&pool).await;
        let token = issue(&mut *conn, user_id, Purpose::VerifyEmail)
            .await
            .expect("発行できるはず")
            .expect("初回は発行されるはず");

        let result = consume(&mut *conn, &token, Purpose::ResetPassword)
            .await
            .expect("照合できるはず");

        assert_eq!(result, None);
    }

    #[sqlx::test]
    async fn consume_rejects_an_expired_token(pool: SqlitePool) {
        let user_id = insert_user(&pool, "kyoko").await;
        let mut conn = conn(&pool).await;
        let token = issue(&mut *conn, user_id, Purpose::ResetPassword)
            .await
            .expect("発行できるはず")
            .expect("初回は発行されるはず");
        sqlx::query!("UPDATE email_tokens SET expires_at = '2000-01-01T00:00:00.000Z'")
            .execute(&mut *conn)
            .await
            .expect("テストデータを更新できるはず");

        let result = consume(&mut *conn, &token, Purpose::ResetPassword)
            .await
            .expect("照合できるはず");

        assert_eq!(result, None);
    }

    /// 確かめるだけでは使用済みにしない。使った後・用途違い・期限切れは使えないと答える。
    #[sqlx::test]
    async fn is_usable_does_not_consume_the_token(pool: SqlitePool) {
        let user_id = insert_user(&pool, "kyoko").await;
        let mut conn = conn(&pool).await;
        let token = issue(&mut *conn, user_id, Purpose::ResetPassword)
            .await
            .expect("発行できるはず")
            .expect("初回は発行されるはず");

        let usable = |purpose| is_usable(&pool, &token, purpose);
        assert!(
            usable(Purpose::ResetPassword)
                .await
                .expect("照合できるはず")
        );
        assert!(
            usable(Purpose::ResetPassword)
                .await
                .expect("照合できるはず")
        );
        assert!(!usable(Purpose::VerifyEmail).await.expect("照合できるはず"));

        consume(&mut *conn, &token, Purpose::ResetPassword)
            .await
            .expect("照合できるはず")
            .expect("確かめた後も使えるはず");
        assert!(
            !usable(Purpose::ResetPassword)
                .await
                .expect("照合できるはず")
        );
    }

    #[sqlx::test]
    async fn is_usable_rejects_an_expired_token(pool: SqlitePool) {
        let user_id = insert_user(&pool, "kyoko").await;
        let token = issue(&pool, user_id, Purpose::VerifyEmail)
            .await
            .expect("発行できるはず")
            .expect("初回は発行されるはず");
        sqlx::query!("UPDATE email_tokens SET expires_at = '2000-01-01T00:00:00.000Z'")
            .execute(&pool)
            .await
            .expect("テストデータを更新できるはず");

        let usable = is_usable(&pool, &token, Purpose::VerifyEmail)
            .await
            .expect("照合できるはず");

        assert!(!usable);
    }

    /// 使われていないトークンを直前に発行済みなら発行し直さず、前のリンクもそのまま使える。
    #[sqlx::test]
    async fn issue_skips_a_recent_unused_token(pool: SqlitePool) {
        let user_id = insert_user(&pool, "kyoko").await;
        let mut conn = conn(&pool).await;
        let first = issue(&mut *conn, user_id, Purpose::ResetPassword)
            .await
            .expect("発行できるはず")
            .expect("初回は発行されるはず");

        let recent = issue(&mut *conn, user_id, Purpose::ResetPassword)
            .await
            .expect("発行できるはず");

        assert_eq!(recent, None);
        let first_result = consume(&mut *conn, &first, Purpose::ResetPassword)
            .await
            .expect("照合できるはず");
        assert_eq!(first_result, Some(user_id));
    }

    /// 発行から間隔が空いていれば発行し直し、前のリンクは使えなくなる。
    #[sqlx::test]
    async fn issue_after_the_cooldown_invalidates_the_previous_token(pool: SqlitePool) {
        let user_id = insert_user(&pool, "kyoko").await;
        let mut conn = conn(&pool).await;
        let old = issue(&mut *conn, user_id, Purpose::ResetPassword)
            .await
            .expect("発行できるはず")
            .expect("初回は発行されるはず");
        backdate_issue(&mut *conn, Purpose::ResetPassword, 10).await;

        let new = issue(&mut *conn, user_id, Purpose::ResetPassword)
            .await
            .expect("発行できるはず")
            .expect("間隔が空いていれば発行されるはず");

        let old_result = consume(&mut *conn, &old, Purpose::ResetPassword)
            .await
            .expect("照合できるはず");
        let new_result = consume(&mut *conn, &new, Purpose::ResetPassword)
            .await
            .expect("照合できるはず");
        assert_eq!(old_result, None, "発行し直したら前のリンクは使えない");
        assert_eq!(new_result, Some(user_id));
    }

    /// 使い済みなら、直前の発行でも発行し直す。
    #[sqlx::test]
    async fn issue_reissues_right_after_the_token_is_used(pool: SqlitePool) {
        let user_id = insert_user(&pool, "kyoko").await;
        let mut conn = conn(&pool).await;
        let first = issue(&mut *conn, user_id, Purpose::ResetPassword)
            .await
            .expect("発行できるはず")
            .expect("初回は発行されるはず");
        consume(&mut *conn, &first, Purpose::ResetPassword)
            .await
            .expect("照合できるはず")
            .expect("使えるはず");

        let second = issue(&mut *conn, user_id, Purpose::ResetPassword)
            .await
            .expect("発行できるはず");

        assert!(second.is_some());
    }

    /// 期限は用途ごとに違う。リセットは1時間、確認は24時間。
    #[sqlx::test]
    async fn issue_sets_the_lifetime_per_purpose(pool: SqlitePool) {
        let user_id = insert_user(&pool, "kyoko").await;
        let mut conn = conn(&pool).await;
        issue(&mut *conn, user_id, Purpose::ResetPassword)
            .await
            .expect("発行できるはず");
        issue(&mut *conn, user_id, Purpose::VerifyEmail)
            .await
            .expect("発行できるはず");

        let lifetimes = sqlx::query!(
            r#"SELECT purpose,
                      CAST(ROUND((julianday(expires_at) - julianday('now')) * 24) AS INTEGER)
                          AS "hours!: i64"
               FROM email_tokens ORDER BY purpose"#
        )
        .fetch_all(&mut *conn)
        .await
        .expect("読めるはず")
        .into_iter()
        .map(|row| (row.purpose, row.hours))
        .collect::<Vec<_>>();

        assert_eq!(
            lifetimes,
            vec![
                ("reset_password".to_string(), 1),
                ("verify_email".to_string(), 24)
            ]
        );
    }
}
