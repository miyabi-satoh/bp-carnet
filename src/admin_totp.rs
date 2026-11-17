//! 管理画面に入るときの確認コード (TOTP、docs/authentication.md「管理者の確認コード」)。
//!
//! 管理者は全員の記録を見られるので、管理 API にはログインに加えて、認証アプリの確認コードを求める。
//! 登録はサーバーの端末のコマンド (`--enroll-admin-code`) だけで行う。画面から登録させると、
//! パスワードを破った相手が先に自分の認証アプリを登録できてしまうため。

use sqlx::SqlitePool;
use totp_rs::{Builder, Secret, Totp};

use crate::config::non_empty_env;
use crate::token_cipher::TokenCipher;

/// 秘密を暗号化する鍵の元。設定されているときだけ、管理 API に確認コードを求める
/// (ほかの外部サービスと同じく、有無で機能ごと切る。docs/architecture.md「機能の有効化条件」)。
pub const KEY_ENV: &str = "ADMIN_TOTP_KEY";

/// 認証アプリに出る発行元の名前。
const ISSUER: &str = "BP Carnet";

/// 一度通した確認コードを、もう一度求めずに済ませる長さ (秒)。管理の作業をひととおり済ませられ、
/// 離席した端末を長く開けておかない長さとしてユーザーが決めた。
const VERIFIED_FOR_SECONDS: i64 = 60 * 60;

pub struct AdminTotp {
    cipher: TokenCipher,
}

// 鍵をログに出さない。
impl std::fmt::Debug for AdminTotp {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AdminTotp").finish_non_exhaustive()
    }
}

/// 端末のコマンドで出す、登録の中身。
pub struct Enrollment {
    totp: Totp,
}

impl Enrollment {
    /// 認証アプリに読ませる URL (`otpauth://`)。QR コードにして出す。
    pub fn url(&self) -> String {
        self.totp
            .to_url()
            .expect("発行元とアカウント名にコロンを含まない")
    }

    /// QR コードを読めないときに、認証アプリに手で入れる設定用の文字列。
    pub fn secret_base32(&self) -> String {
        self.totp.secret().to_base32()
    }

    /// 認証アプリに出た6桁が合っていれば、その時間の区切りを返す。
    pub fn check(&self, code: &str) -> Option<u64> {
        check(&self.totp, code)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("確認コードの登録を読めません")]
    Db(#[from] sqlx::Error),
    /// 鍵が変わった・中身が壊れたときは、登録し直してもらう。
    #[error("確認コードの登録を復号できません。端末で登録し直してください")]
    Unreadable,
}

/// [`AdminTotp::verify`] の結果。
#[derive(Debug, PartialEq, Eq)]
pub enum Verification {
    Passed,
    WrongCode,
    NotEnrolled,
}

impl AdminTotp {
    /// 環境変数に鍵があれば作る。無ければ確認コードを求めない。
    pub fn from_env() -> Option<Self> {
        non_empty_env(KEY_ENV).map(|key| Self::with_key(&key))
    }

    pub fn with_key(key: &str) -> Self {
        Self {
            cipher: TokenCipher::derive("admin-totp", key),
        }
    }

    /// 新しい秘密で登録の中身を作る。`username` は認証アプリに出るアカウント名。
    pub fn new_enrollment(username: &str) -> Enrollment {
        Enrollment {
            totp: totp(Secret::generate(), username),
        }
    }

    /// 登録する (登録し直すと前のものは無効になる)。`step` は登録の確かめに通したコードの区切りで、
    /// 同じコードで管理画面に入れないよう、使ったものとして残す。
    pub async fn save(
        &self,
        pool: &SqlitePool,
        user_id: i64,
        enrollment: &Enrollment,
        step: u64,
    ) -> Result<(), sqlx::Error> {
        let sealed = self.cipher.seal(&enrollment.secret_base32());
        let step = stored_step(step);
        sqlx::query!(
            "INSERT INTO admin_totp (user_id, secret, last_step) VALUES (?, ?, ?)
             ON CONFLICT (user_id) DO UPDATE SET
                 secret = excluded.secret,
                 last_step = excluded.last_step,
                 created_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')",
            user_id,
            sealed,
            step
        )
        .execute(pool)
        .await?;
        Ok(())
    }

    pub async fn is_enrolled(pool: &SqlitePool, user_id: i64) -> Result<bool, sqlx::Error> {
        sqlx::query_scalar!(
            r#"SELECT EXISTS (SELECT 1 FROM admin_totp WHERE user_id = ?) AS "enrolled!: bool""#,
            user_id
        )
        .fetch_one(pool)
        .await
    }

    /// 確認コードを照合する。一度通したコード (同じ時間の区切りか、それより前) は通さない
    /// (RFC 6238 5.2。見られたコードをすぐ使い回されないため)。
    pub async fn verify(
        &self,
        pool: &SqlitePool,
        user_id: i64,
        username: &str,
        code: &str,
    ) -> Result<Verification, Error> {
        let Some(sealed) =
            sqlx::query_scalar!("SELECT secret FROM admin_totp WHERE user_id = ?", user_id)
                .fetch_optional(pool)
                .await?
        else {
            return Ok(Verification::NotEnrolled);
        };
        let secret = self
            .cipher
            .open(&sealed)
            .and_then(|base32| Secret::try_from_base32(base32).ok())
            .ok_or(Error::Unreadable)?;
        let Some(step) = check(&totp(secret, username), code) else {
            return Ok(Verification::WrongCode);
        };
        let step = stored_step(step);
        // 照合と使った印を1文にまとめ、同じコードの同時の送信を両方通さない。
        let updated = sqlx::query!(
            "UPDATE admin_totp SET last_step = ?
             WHERE user_id = ? AND (last_step IS NULL OR last_step < ?)",
            step,
            user_id,
            step
        )
        .execute(pool)
        .await?;
        Ok(if updated.rows_affected() == 1 {
            Verification::Passed
        } else {
            Verification::WrongCode
        })
    }
}

/// 認証アプリに出た6桁が合っていれば、その時間の区切りを返す。前後の空白は無視する。
fn check(totp: &Totp, code: &str) -> Option<u64> {
    totp.check_current(code.trim())
}

/// 時間の区切りを、DB に置ける形にする。今の時刻の区切りは i64 に収まるので、丸めは起きない。
fn stored_step(step: u64) -> i64 {
    i64::try_from(step).unwrap_or(i64::MAX)
}

/// RFC 6238 の既定 (SHA-1・6桁・30秒・前後1つの区切りを許す)。認証アプリの多くがこれだけに対応する。
fn totp(secret: Secret, username: &str) -> Totp {
    Builder::new()
        .with_secret(secret)
        .with_issuer(Some(ISSUER))
        .with_account_name(username.replace(':', ""))
        .build()
        .expect("RFC の既定の値と、160ビットの秘密")
}

/// ウェブのセッションに持つ、確認コードを通した時刻 (Unix 秒)。アプリのトークンは `app_tokens.admin_verified_at`。
const SESSION_VERIFIED_AT_KEY: &str = "admin_verified_at";

/// 確認コードを通した印を置く先。ログインの証しごとに持ち、ほかの端末のログインには効かせない。
pub(crate) enum VerificationTarget<'a> {
    Session(&'a tower_sessions::Session),
    AppToken(i64),
}

impl<'a> VerificationTarget<'a> {
    /// アプリのトークンでのログインならトークン、ウェブならセッション。
    pub(crate) fn of(user: &crate::auth::AuthUser, session: &'a tower_sessions::Session) -> Self {
        match user.app_token_id {
            Some(id) => VerificationTarget::AppToken(id),
            None => VerificationTarget::Session(session),
        }
    }
}

/// ウェブのセッションから、確認コードを通した印を消す。
pub(crate) async fn forget_session_verification(
    session: &tower_sessions::Session,
) -> Result<(), crate::auth::Error> {
    session
        .remove::<i64>(SESSION_VERIFIED_AT_KEY)
        .await
        .map_err(crate::auth::Error::Session)?;
    Ok(())
}

/// このログインが、確認コードを通してから [`VERIFIED_FOR_SECONDS`] の内か。
pub(crate) async fn is_verified(
    pool: &SqlitePool,
    target: VerificationTarget<'_>,
) -> Result<bool, crate::error::AppError> {
    let verified_at: Option<i64> = match target {
        VerificationTarget::Session(session) => session
            .get(SESSION_VERIFIED_AT_KEY)
            .await
            .map_err(crate::auth::Error::Session)?,
        VerificationTarget::AppToken(id) => {
            sqlx::query_scalar!("SELECT admin_verified_at FROM app_tokens WHERE id = ?", id)
                .fetch_optional(pool)
                .await?
                .flatten()
        }
    };
    let now = jiff::Timestamp::now().as_second();
    Ok(verified_at.is_some_and(|at| (0..VERIFIED_FOR_SECONDS).contains(&(now - at))))
}

/// このログインに、確認コードを通した印を置く。
pub(crate) async fn mark_verified(
    pool: &SqlitePool,
    target: VerificationTarget<'_>,
) -> Result<(), crate::error::AppError> {
    let now = jiff::Timestamp::now().as_second();
    match target {
        VerificationTarget::Session(session) => session
            .insert(SESSION_VERIFIED_AT_KEY, now)
            .await
            .map_err(crate::auth::Error::Session)?,
        VerificationTarget::AppToken(id) => {
            sqlx::query!(
                "UPDATE app_tokens SET admin_verified_at = ? WHERE id = ?",
                now,
                id
            )
            .execute(pool)
            .await?;
        }
    }
    Ok(())
}

#[cfg(test)]
pub(crate) fn code_now(enrollment: &Enrollment) -> String {
    enrollment.totp.generate_current().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::insert_user;

    async fn enrolled(pool: &SqlitePool) -> (AdminTotp, i64, Enrollment) {
        let user_id = insert_user(pool, "admin@example.com").await;
        let totp = AdminTotp::with_key("test-key");
        let enrollment = AdminTotp::new_enrollment("admin@example.com");
        totp.save(pool, user_id, &enrollment, 0)
            .await
            .expect("登録できる");
        (totp, user_id, enrollment)
    }

    #[sqlx::test]
    async fn verify_accepts_the_current_code_only_once(pool: SqlitePool) {
        let (totp, user_id, enrollment) = enrolled(&pool).await;
        let code = code_now(&enrollment);

        assert_eq!(
            totp.verify(&pool, user_id, "admin@example.com", &code)
                .await
                .expect("照合できる"),
            Verification::Passed
        );
        assert_eq!(
            totp.verify(&pool, user_id, "admin@example.com", &code)
                .await
                .expect("照合できる"),
            Verification::WrongCode
        );
    }

    #[sqlx::test]
    async fn verify_rejects_a_wrong_code_and_reports_a_missing_enrollment(pool: SqlitePool) {
        let (totp, user_id, enrollment) = enrolled(&pool).await;
        let wrong = if code_now(&enrollment) == "000000" {
            "111111"
        } else {
            "000000"
        };
        assert_eq!(
            totp.verify(&pool, user_id, "admin@example.com", wrong)
                .await
                .expect("照合できる"),
            Verification::WrongCode
        );

        let other = insert_user(&pool, "other@example.com").await;
        assert_eq!(
            totp.verify(&pool, other, "other@example.com", "123456")
                .await
                .expect("照合できる"),
            Verification::NotEnrolled
        );
    }

    /// 鍵が変わると、前の登録は読めない (登録し直してもらう)。
    #[sqlx::test]
    async fn verify_reports_an_unreadable_secret_after_the_key_changes(pool: SqlitePool) {
        let (_, user_id, enrollment) = enrolled(&pool).await;
        let other_key = AdminTotp::with_key("another-key");
        let result = other_key
            .verify(&pool, user_id, "admin@example.com", &code_now(&enrollment))
            .await;
        assert!(matches!(result, Err(Error::Unreadable)));
    }

    /// 登録の確かめに使ったコードでは、管理画面に入れない。
    #[sqlx::test]
    async fn the_code_used_to_enroll_cannot_be_reused(pool: SqlitePool) {
        let user_id = insert_user(&pool, "admin@example.com").await;
        let totp = AdminTotp::with_key("test-key");
        let enrollment = AdminTotp::new_enrollment("admin@example.com");
        let code = code_now(&enrollment);
        let step = enrollment.check(&code).expect("今のコードは合う");
        totp.save(&pool, user_id, &enrollment, step)
            .await
            .expect("登録できる");

        assert_eq!(
            totp.verify(&pool, user_id, "admin@example.com", &code)
                .await
                .expect("照合できる"),
            Verification::WrongCode
        );
    }

    #[test]
    fn the_url_names_the_issuer_and_the_account() {
        let enrollment = AdminTotp::new_enrollment("admin@example.com");
        let url = enrollment.url();
        assert!(
            url.starts_with("otpauth://totp/BP%20Carnet:admin%40example.com?"),
            "{url}"
        );
        assert!(
            url.contains(&format!("secret={}", enrollment.secret_base32())),
            "{url}"
        );
    }
}
