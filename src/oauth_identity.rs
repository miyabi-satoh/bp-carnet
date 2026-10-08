//! `oauth_identities` (Google・LINE・Apple のアカウントとの紐付け) の読み書き。
//!
//! 外部のアカウントでのログインが行き着くユーザーを決める処理を、識別の検索・既存アカウントへの
//! 自動統合・新規作成の順にここへ集約する。各プロバイダーとの通信は `crate::google_login` (Google)・
//! `crate::line_login` (LINE)・`crate::apple_login` (Apple) が扱う。

use sqlx::SqlitePool;

use crate::apple_login::{self, AppleIdToken};
use crate::auth::{self, Role};
use crate::google_login::GoogleIdToken;
use crate::line_login::LineIdToken;

/// ログインに使う外部のアカウントの提供元 (`oauth_identities.provider`)。
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, utoipa::ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum Provider {
    Google,
    Line,
    Apple,
}

impl Provider {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Google => "google",
            Self::Line => "line",
            Self::Apple => "apple",
        }
    }

    /// `as_str` の逆。未知の名前は `None`。
    pub fn parse(name: &str) -> Option<Self> {
        match name {
            "google" => Some(Self::Google),
            "line" => Some(Self::Line),
            "apple" => Some(Self::Apple),
            _ => None,
        }
    }
}

/// プロバイダーから受け取った、ログインする人のアカウント。
#[derive(Debug)]
pub struct ExternalAccount {
    pub provider: Provider,
    /// プロバイダーの中で変わらない識別子 (各プロバイダーの `sub`)。
    pub subject: String,
    pub email: Option<String>,
    pub name: Option<String>,
    pub picture: Option<String>,
}

impl From<GoogleIdToken> for ExternalAccount {
    fn from(info: GoogleIdToken) -> Self {
        Self {
            provider: Provider::Google,
            subject: info.sub,
            email: info.email,
            name: info.name,
            picture: info.picture,
        }
    }
}

impl ExternalAccount {
    /// Apple は名前を ID トークンに載せず、最初の承認のときだけ別に渡す (docs/authentication.md)。
    pub fn from_apple(
        token: AppleIdToken,
        given_name: Option<&str>,
        family_name: Option<&str>,
    ) -> Self {
        Self {
            provider: Provider::Apple,
            subject: token.sub,
            email: token.email,
            name: apple_login::display_name(given_name, family_name),
            picture: None,
        }
    }
}

impl From<LineIdToken> for ExternalAccount {
    fn from(token: LineIdToken) -> Self {
        Self {
            provider: Provider::Line,
            subject: token.sub,
            email: token.email,
            name: token.name,
            picture: token.picture,
        }
    }
}

/// このログインが行き着く既存ユーザーが凍結中なら、その `user_id`。凍結中でない場合と、
/// まだ存在しない場合 (これから作られるユーザーは凍結されていない) は `None`。
///
/// `find_or_create_user` と同じ順序で探す (識別 → 同じメールアドレスの既存ユーザー)。
pub async fn frozen_target(
    pool: &SqlitePool,
    account: &ExternalAccount,
) -> Result<Option<i64>, sqlx::Error> {
    let user_id = match find_identity(pool, account.provider, &account.subject).await? {
        Some(user_id) => Some(user_id),
        None => match account.email.as_deref() {
            Some(email) => {
                let username = auth::normalize_username(email);
                sqlx::query_scalar!(
                    r#"SELECT id AS "id!: i64" FROM users WHERE username = ?"#,
                    username
                )
                .fetch_optional(pool)
                .await?
            }
            None => None,
        },
    };

    let Some(user_id) = user_id else {
        return Ok(None);
    };
    let frozen = sqlx::query_scalar!("SELECT frozen FROM users WHERE id = ?", user_id)
        .fetch_one(pool)
        .await?;
    Ok((frozen != 0).then_some(user_id))
}

#[derive(Debug, thiserror::Error)]
pub enum FindOrCreateError {
    /// `username` (= email) が、自動統合できない既存アカウント (同じプロバイダーの別の
    /// アカウントと連携済み) に既に使われていた。
    #[error("username already taken by an account that cannot be linked")]
    UsernameConflict,
    /// プロバイダーが `email` を返さなかった (Google はスコープ不足等で通常は起こらない。
    /// LINE は呼び出し元が先に弾く)。
    #[error("the provider did not return an email address")]
    MissingEmail,
    #[error(transparent)]
    Db(#[from] sqlx::Error),
    #[error(transparent)]
    Hash(#[from] auth::Error),
}

/// 外部のアカウントでログインするユーザーを次の順に探し、その `user_id` を返す。
///
/// 1. `oauth_identities` を `(provider, subject)` で検索する
/// 2. 同じメールアドレスの既存ユーザーに紐付ける (自動統合)
/// 3. 新規に `users`/`oauth_identities` を作成する
pub async fn find_or_create_user(
    pool: &SqlitePool,
    account: &ExternalAccount,
    terms_version: &str,
) -> Result<i64, FindOrCreateError> {
    let provider = account.provider.as_str();
    let subject = account.subject.as_str();
    let email = account
        .email
        .as_deref()
        .ok_or(FindOrCreateError::MissingEmail)?;
    // プロバイダーが返す email は大文字を含みうる。`users.username` は常に正規化した値で
    // 保存・照合する (docs/data-model.md)。正規化しないと、大文字違いの同じアドレスが
    // UNIQUE 制約をすり抜けて別アカウントとして作られてしまう。
    let username = auth::normalize_username(email);

    if let Some(user_id) = find_identity(pool, account.provider, subject).await? {
        return Ok(user_id);
    }

    if let Some(user_id) = link_identity_to_existing_user(
        pool,
        account.provider,
        &username,
        subject,
        email,
        terms_version,
    )
    .await?
    {
        return Ok(user_id);
    }

    // argon2 のハッシュ化 (CPU バウンド、spawn_blocking) はトランザクション開始前に済ませ、
    // SQLite の書き込みトランザクションをその間保持しないようにする。
    let unusable_hash = auth::unusable_password_hash().await?;

    let mut tx = pool.begin().await?;

    // メールアドレスの確認は呼び出し元が済ませている (Google は持ち主を保証するメールか
    // (`GoogleIdToken::email_is_trusted`)。LINE は
    // 確認済みかを返さないが、そのまま信じると決めた: docs/authentication.md)。
    let insert_user = auth::create_user(
        &mut *tx,
        &username,
        auth::NewPassword::Unusable(&unusable_hash),
        Role::User,
    )
    .await;

    let user_id = match insert_user {
        Ok(user_id) => user_id,
        Err(sqlx::Error::Database(db_err)) if db_err.is_unique_violation() => {
            // username(=email) の UNIQUE 違反には2パターンありうる:
            //  (a) 自動統合できない既存アカウント (同じプロバイダーの別のアカウントと連携済み) が
            //      同じ email を使っていた → 本当の衝突。エラーにする。
            //  (b) 同時に同じ sub で2回コールバックが飛んできたレース (二重クリック・
            //      複数タブ) → 直前の INSERT が commit されていれば、再度
            //      oauth_identities を引き直せば見つかるはず。
            tx.rollback().await.ok();
            return match find_identity(pool, account.provider, subject).await? {
                Some(user_id) => Ok(user_id),
                None => Err(FindOrCreateError::UsernameConflict),
            };
        }
        Err(err) => return Err(err.into()),
    };

    record_terms_agreement(&mut tx, user_id, terms_version).await?;

    // `oauth_identities.email` は検索キーではなくプロバイダー側の情報の記録用なので、
    // 正規化せず受け取ったままの値を保存する。
    let insert_identity = sqlx::query!(
        "INSERT INTO oauth_identities (user_id, provider, subject, email) VALUES (?, ?, ?, ?)",
        user_id,
        provider,
        subject,
        email
    )
    .execute(&mut *tx)
    .await;

    match insert_identity {
        Ok(_) => {
            tx.commit().await?;
            Ok(user_id)
        }
        Err(sqlx::Error::Database(db_err)) if db_err.is_unique_violation() => {
            // `oauth_identities` 側の UNIQUE(provider, subject) 違反。username(=email) が
            // 一致しない (例: 同時に同じ sub で2回コールバックが飛んできた間にプロバイダー側で
            // メールアドレスが変わった) ため users 側の UNIQUE 違反では検知できなかった、
            // 同じ sub への同時ログインのレース。トランザクション全体 (この users INSERT 分も
            // 含む) をロールバックし、先に commit されたはずの識別を引き直す。
            tx.rollback().await.ok();
            match find_identity(pool, account.provider, subject).await? {
                Some(user_id) => Ok(user_id),
                // SQLite は書き込みを直列化するため、UNIQUE 違反が起きた時点で競合相手の
                // トランザクションは commit 済みのはずで、通常はここに到達しない。
                None => Err(FindOrCreateError::UsernameConflict),
            }
        }
        Err(err) => Err(err.into()),
    }
}

/// `username` の既存ユーザーに外部のアカウントの識別を紐付け、その `user_id` を返す。統合できる
/// ユーザーがいなければ `None`。
///
/// ADR: 紐付けるのは、メール確認済みでまだそのプロバイダーと連携していないユーザーだけ。連携済みの
/// ユーザーに別の `sub` を足さないのは、Google Workspace 等では同じメールアドレスが別人の
/// 新しいアカウント (別の `sub`) に割り当て直されうるため。条件の確認と INSERT を1文にまとめ、
/// 別々の `sub` からの同時ログインが両方とも紐付くのを防ぐ。
async fn link_identity_to_existing_user(
    pool: &SqlitePool,
    provider: Provider,
    username: &str,
    subject: &str,
    email: &str,
    terms_version: &str,
) -> Result<Option<i64>, sqlx::Error> {
    // 未確認のアカウント (`crate::signup`) はパスワードも記録も持たないため、同じメールアドレスの
    // Google・LINE・Apple のログインが来たら確認済みにして引き継ぐ。紐付けが別の `sub` との競合で
    // 成立しなくても、確認済みのまま残って困ることは無い。
    // 引き継ぎは外部のアカウントでの登録にあたるので、同意もこのときの版で記録し直す。
    sqlx::query!(
        "UPDATE users
         SET email_verified = 1,
             terms_version = ?1,
             terms_agreed_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
         WHERE username = ?2 AND email_verified = 0",
        terms_version,
        username
    )
    .execute(pool)
    .await?;

    let provider = provider.as_str();
    let linked = sqlx::query!(
        r#"INSERT INTO oauth_identities (user_id, provider, subject, email)
           SELECT id, ?1, ?2, ?3 FROM users
           WHERE username = ?4
             AND email_verified = 1
             AND NOT EXISTS (
                 SELECT 1 FROM oauth_identities
                 WHERE oauth_identities.user_id = users.id AND oauth_identities.provider = ?1
             )
           RETURNING user_id AS "user_id!: i64""#,
        provider,
        subject,
        email,
        username
    )
    .fetch_optional(pool)
    .await;

    match linked {
        Ok(row) => Ok(row.map(|r| r.user_id)),
        // 同じ `sub` の並行コールバックが先に識別を作っていた。呼び出し側の新規作成の経路が
        // UNIQUE 違反から識別を引き直して回復する。
        Err(sqlx::Error::Database(db_err)) if db_err.is_unique_violation() => Ok(None),
        Err(err) => Err(err),
    }
}

/// 外部のアカウントで作ったアカウントに、同意した規約の版と日時を記録する。
async fn record_terms_agreement(
    conn: &mut sqlx::SqliteConnection,
    user_id: i64,
    terms_version: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query!(
        "UPDATE users SET terms_version = ?, terms_agreed_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
         WHERE id = ?",
        terms_version,
        user_id
    )
    .execute(conn)
    .await?;
    Ok(())
}

/// プロフィール (名前・画像) を、ログインの都度最新の値で上書きする (docs/authentication.md)。
/// 複数の提供元と連携していれば、後からログインした方の値になる。
/// プロバイダー側で消された値は `NULL` に戻す。
pub async fn save_profile(
    pool: &SqlitePool,
    user_id: i64,
    account: &ExternalAccount,
) -> Result<(), sqlx::Error> {
    let display_name = account
        .name
        .as_deref()
        .map(str::trim)
        .filter(|name| !name.is_empty());
    // Apple は名前を最初の承認のときにしか渡さず、画像も無い。渡ったときだけ名前を入れ、
    // それ以外のログインでは何も変えない (docs/authentication.md)。
    if account.provider == Provider::Apple {
        if let Some(display_name) = display_name {
            sqlx::query!(
                "UPDATE users SET display_name = ? WHERE id = ?",
                display_name,
                user_id
            )
            .execute(pool)
            .await?;
        }
        return Ok(());
    }
    // 画像の URL はそのまま `<img src>` に使うため、ホストを持つ https の URL だけを受け付ける。
    let avatar_url = account
        .picture
        .as_deref()
        .and_then(|url| reqwest::Url::parse(url).ok())
        .filter(|url| url.scheme() == "https" && url.host_str().is_some())
        .map(String::from);
    sqlx::query!(
        "UPDATE users SET display_name = ?, avatar_url = ? WHERE id = ?",
        display_name,
        avatar_url,
        user_id
    )
    .execute(pool)
    .await?;
    Ok(())
}

/// 提供元との連携を取り消すためのトークン (LINE・Apple。どちらも暗号化したもの)。
///
/// LINE は、ウェブのログインでリフレッシュトークンを、アプリのログインでアクセストークンを受け取る
/// (iOS の LINE SDK はリフレッシュトークンを渡さない、docs/mobile-app.md)。Apple はリフレッシュトークンだけ。
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct RevocationTokens {
    pub refresh_token: Option<String>,
    pub access_token: Option<String>,
}

impl RevocationTokens {
    pub fn is_empty(&self) -> bool {
        self.refresh_token.is_none() && self.access_token.is_none()
    }
}

/// 連携を取り消すためのトークンを、ログインのたびに最新の値で保存する。受け取らなかった方は
/// 前の値を残す (ウェブとアプリの両方でログインする人は、どちらかが期限内なら取り消せるため)。
pub async fn save_revocation_tokens(
    pool: &SqlitePool,
    provider: Provider,
    subject: &str,
    tokens: &RevocationTokens,
) -> Result<(), sqlx::Error> {
    let provider = provider.as_str();
    sqlx::query!(
        "UPDATE oauth_identities
         SET refresh_token = COALESCE(?, refresh_token), access_token = COALESCE(?, access_token)
         WHERE provider = ? AND subject = ?",
        tokens.refresh_token,
        tokens.access_token,
        provider,
        subject
    )
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn find_identity(
    pool: &SqlitePool,
    provider: Provider,
    subject: &str,
) -> Result<Option<i64>, sqlx::Error> {
    let provider = provider.as_str();
    let row = sqlx::query!(
        r#"SELECT user_id AS "user_id!: i64" FROM oauth_identities WHERE provider = ? AND subject = ?"#,
        provider,
        subject
    )
    .fetch_optional(pool)
    .await?;
    Ok(row.map(|r| r.user_id))
}

/// ユーザーが連携している提供元。設定画面の「ログイン方法」に並べる。
pub async fn linked_providers(
    pool: &SqlitePool,
    user_id: i64,
) -> Result<Vec<Provider>, sqlx::Error> {
    let rows = sqlx::query_scalar!(
        "SELECT provider FROM oauth_identities WHERE user_id = ? ORDER BY id",
        user_id
    )
    .fetch_all(pool)
    .await?;
    // provider の値は 'google' | 'line' | 'apple' だけ (docs/data-model.md)。
    Ok(rows
        .iter()
        .filter_map(|name| Provider::parse(name))
        .collect())
}

/// [`unlink`] の結果。
#[derive(Debug, PartialEq, Eq)]
pub enum Unlinked {
    /// 連携を外した。LINE・Apple なら、取り消しに使うトークンを返す。
    Done { revocation_tokens: RevocationTokens },
    /// その提供元とは連携していない。
    NotLinked,
    /// 外すとログイン方法が残らない (docs/authentication.md)。
    LastLoginMethod,
}

/// 連携を外す。外した後もログインできる方法 (パスワード・ほかの連携) が残るときだけ。
///
/// 残るかの判定と削除は1文で行う。判定してから消すと、ほかの連携を同時に外す2つの
/// リクエストが両方通り、ログイン方法が無くなりうるため。
pub async fn unlink(
    pool: &SqlitePool,
    user_id: i64,
    provider: Provider,
) -> Result<Unlinked, sqlx::Error> {
    let provider = provider.as_str();
    let deleted = sqlx::query!(
        r#"DELETE FROM oauth_identities
           WHERE user_id = ?1 AND provider = ?2
             AND ((SELECT password_usable FROM users WHERE id = ?1) = 1
                  OR EXISTS (SELECT 1 FROM oauth_identities
                             WHERE user_id = ?1 AND provider <> ?2))
           RETURNING refresh_token, access_token"#,
        user_id,
        provider
    )
    .fetch_optional(pool)
    .await?;
    if let Some(deleted) = deleted {
        return Ok(Unlinked::Done {
            revocation_tokens: RevocationTokens {
                refresh_token: deleted.refresh_token,
                access_token: deleted.access_token,
            },
        });
    }

    let linked = sqlx::query_scalar!(
        r#"SELECT COUNT(*) AS "count!: i64" FROM oauth_identities WHERE user_id = ? AND provider = ?"#,
        user_id,
        provider
    )
    .fetch_one(pool)
    .await?;
    Ok(if linked == 0 {
        Unlinked::NotLinked
    } else {
        Unlinked::LastLoginMethod
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::insert_user;

    const PROVIDERS: [Provider; 3] = [Provider::Google, Provider::Line, Provider::Apple];
    const TERMS_VERSION: &str = "2026-01-01";

    fn account(provider: Provider, subject: &str, email: &str) -> ExternalAccount {
        ExternalAccount {
            provider,
            subject: subject.to_owned(),
            email: Some(email.to_owned()),
            name: None,
            picture: None,
        }
    }

    /// 提供元ごとに別のメールアドレスにして、同じ DB の中でぶつからないようにする。
    fn email_for(provider: Provider, local: &str) -> String {
        format!("{local}-{}@example.com", provider.as_str())
    }

    async fn find_or_create(
        pool: &SqlitePool,
        account: &ExternalAccount,
    ) -> Result<i64, FindOrCreateError> {
        find_or_create_user(pool, account, TERMS_VERSION).await
    }

    async fn count_users(pool: &SqlitePool) -> i64 {
        sqlx::query_scalar!(r#"SELECT COUNT(*) AS "count!: i64" FROM users"#)
            .fetch_one(pool)
            .await
            .expect("users should be countable")
    }

    async fn count_identities(pool: &SqlitePool, provider: Provider) -> i64 {
        let provider = provider.as_str();
        sqlx::query_scalar!(
            r#"SELECT COUNT(*) AS "count!: i64" FROM oauth_identities WHERE provider = ?"#,
            provider
        )
        .fetch_one(pool)
        .await
        .expect("identities should be countable")
    }

    struct UserRow {
        username: String,
        password_hash: String,
        password_usable: bool,
        email_verified: bool,
        terms_version: Option<String>,
        terms_agreed_at: Option<String>,
        display_name: Option<String>,
        avatar_url: Option<String>,
    }

    async fn user_row(pool: &SqlitePool, user_id: i64) -> UserRow {
        let row = sqlx::query!(
            r#"SELECT username, password_hash, password_usable AS "password_usable: bool",
                      email_verified AS "email_verified: bool", terms_version, terms_agreed_at,
                      display_name, avatar_url
               FROM users WHERE id = ?"#,
            user_id
        )
        .fetch_one(pool)
        .await
        .expect("user should exist");
        UserRow {
            username: row.username,
            password_hash: row.password_hash,
            password_usable: row.password_usable,
            email_verified: row.email_verified,
            terms_version: row.terms_version,
            terms_agreed_at: row.terms_agreed_at,
            display_name: row.display_name,
            avatar_url: row.avatar_url,
        }
    }

    async fn freeze(pool: &SqlitePool, user_id: i64) {
        sqlx::query!("UPDATE users SET frozen = 1 WHERE id = ?", user_id)
            .execute(pool)
            .await
            .expect("user should be frozen");
    }

    #[sqlx::test]
    async fn first_login_creates_a_user_with_the_lowercased_email(pool: SqlitePool) {
        for provider in PROVIDERS {
            let email = format!("New.User-{}@Example.COM", provider.as_str());
            let user_id = find_or_create(&pool, &account(provider, "sub-1", &email))
                .await
                .expect("a user should be created");

            let user = user_row(&pool, user_id).await;
            assert_eq!(user.username, email.to_ascii_lowercase(), "{provider:?}");
            assert!(!user.password_usable, "{provider:?}");
            assert!(user.email_verified, "{provider:?}");
            assert_eq!(
                user.terms_version.as_deref(),
                Some(TERMS_VERSION),
                "{provider:?}"
            );
            assert!(user.terms_agreed_at.is_some(), "{provider:?}");
            assert_eq!(
                find_identity(&pool, provider, "sub-1")
                    .await
                    .expect("identity should be readable"),
                Some(user_id),
                "{provider:?}"
            );
            // 識別に残すメールは、提供元から受け取ったままの値。
            let stored_email = sqlx::query_scalar!(
                "SELECT email FROM oauth_identities WHERE user_id = ?",
                user_id
            )
            .fetch_one(&pool)
            .await
            .expect("identity should exist");
            assert_eq!(stored_email, email, "{provider:?}");
        }
    }

    #[sqlx::test]
    async fn repeat_login_returns_the_same_user(pool: SqlitePool) {
        for provider in PROVIDERS {
            let login = account(provider, "sub-1", &email_for(provider, "repeat"));
            let first = find_or_create(&pool, &login).await.expect("first login");
            let second = find_or_create(&pool, &login).await.expect("second login");
            assert_eq!(first, second, "{provider:?}");
            assert_eq!(count_identities(&pool, provider).await, 1, "{provider:?}");
        }
        assert_eq!(count_users(&pool).await, 3);
    }

    /// 管理者が作ったアカウントに、大文字小文字の違うメールでも紐付ける。パスワードと同意は変えない。
    #[sqlx::test]
    async fn login_links_a_verified_account_with_the_same_email_ignoring_case(pool: SqlitePool) {
        for provider in PROVIDERS {
            let email = email_for(provider, "shared");
            let user_id = insert_user(&pool, &email).await;
            let before = user_row(&pool, user_id).await;

            let linked = find_or_create(&pool, &account(provider, "sub-1", &email.to_uppercase()))
                .await
                .expect("the account should be linked");

            assert_eq!(linked, user_id, "{provider:?}");
            let after = user_row(&pool, user_id).await;
            assert_eq!(after.password_hash, before.password_hash, "{provider:?}");
            assert!(after.password_usable, "{provider:?}");
            assert_eq!(after.terms_version, before.terms_version, "{provider:?}");
            assert_eq!(
                after.terms_agreed_at, before.terms_agreed_at,
                "{provider:?}"
            );
        }
        assert_eq!(count_users(&pool).await, 3);
    }

    /// 別の提供元と連携済みのアカウントにも、同じメールなら足せる。
    #[sqlx::test]
    async fn login_links_an_account_linked_to_other_providers(pool: SqlitePool) {
        let email = "shared@example.com";
        let user_id = find_or_create(&pool, &account(Provider::Google, "g-1", email))
            .await
            .expect("google login");
        for provider in [Provider::Line, Provider::Apple] {
            let linked = find_or_create(&pool, &account(provider, "sub-1", email))
                .await
                .expect("the account should be linked");
            assert_eq!(linked, user_id, "{provider:?}");
        }
        assert_eq!(
            linked_providers(&pool, user_id)
                .await
                .expect("providers should be readable"),
            PROVIDERS
        );
        assert_eq!(count_users(&pool).await, 1);
    }

    /// 同じ提供元の別のアカウント (別の `sub`) と連携済みなら、メールが同じでも紐付けない。
    #[sqlx::test]
    async fn login_does_not_link_an_account_linked_to_another_subject(pool: SqlitePool) {
        for provider in PROVIDERS {
            let email = email_for(provider, "reassigned");
            find_or_create(&pool, &account(provider, "sub-original", &email))
                .await
                .expect("first login");

            let result = find_or_create(&pool, &account(provider, "sub-new", &email)).await;

            assert!(
                matches!(result, Err(FindOrCreateError::UsernameConflict)),
                "{provider:?}: {result:?}"
            );
            assert_eq!(
                find_identity(&pool, provider, "sub-new")
                    .await
                    .expect("identity should be readable"),
                None,
                "{provider:?}"
            );
        }
    }

    /// 未確認のサインアップ (`crate::signup`) は、外部のアカウントでの登録として引き継ぎ、同意も記録し直す。
    #[sqlx::test]
    async fn login_takes_over_an_unverified_signup(pool: SqlitePool) {
        for provider in PROVIDERS {
            let email = email_for(provider, "unverified");
            let user_id = insert_user(&pool, &email).await;
            sqlx::query!("UPDATE users SET email_verified = 0 WHERE id = ?", user_id)
                .execute(&pool)
                .await
                .expect("user should be unverified");

            let linked = find_or_create(&pool, &account(provider, "sub-1", &email))
                .await
                .expect("the signup should be taken over");

            assert_eq!(linked, user_id, "{provider:?}");
            let user = user_row(&pool, user_id).await;
            assert!(user.email_verified, "{provider:?}");
            assert_eq!(
                user.terms_version.as_deref(),
                Some(TERMS_VERSION),
                "{provider:?}"
            );
            assert!(user.terms_agreed_at.is_some(), "{provider:?}");
        }
    }

    #[sqlx::test]
    async fn login_without_an_email_creates_nothing(pool: SqlitePool) {
        for provider in PROVIDERS {
            let mut login = account(provider, "sub-1", "unused@example.com");
            login.email = None;
            let result = find_or_create(&pool, &login).await;
            assert!(
                matches!(result, Err(FindOrCreateError::MissingEmail)),
                "{provider:?}: {result:?}"
            );
        }
        assert_eq!(count_users(&pool).await, 0);
    }

    /// 別々の `sub` から同じアカウントへの同時の初回ログインは、一方だけが紐付く。
    #[sqlx::test]
    async fn only_one_of_concurrent_logins_with_different_subjects_is_linked(pool: SqlitePool) {
        for provider in PROVIDERS {
            let email = email_for(provider, "shared");
            insert_user(&pool, &email).await;

            let first = account(provider, "sub-a", &email);
            let second = account(provider, "sub-b", &email);
            let (a, b) = tokio::join!(
                find_or_create(&pool, &first),
                find_or_create(&pool, &second)
            );

            let conflicts = [&a, &b]
                .iter()
                .filter(|r| matches!(r, Err(FindOrCreateError::UsernameConflict)))
                .count();
            assert_eq!(conflicts, 1, "{provider:?}: {a:?} {b:?}");
            assert!(a.is_ok() || b.is_ok(), "{provider:?}: {a:?} {b:?}");
            assert_eq!(count_identities(&pool, provider).await, 1, "{provider:?}");
        }
        assert_eq!(count_users(&pool).await, 3);
    }

    /// 同じ `sub` の同時のログインで、メールが違う (提供元でメールが変わった直後) と `users` の UNIQUE では
    /// ぶつからず、`oauth_identities` の UNIQUE で初めてぶつかる。両方とも同じユーザーに行き着く。
    #[sqlx::test]
    async fn concurrent_logins_of_the_same_subject_with_different_emails_reach_one_user(
        pool: SqlitePool,
    ) {
        for provider in PROVIDERS {
            let first = account(provider, "sub-race", &email_for(provider, "first"));
            let second = account(provider, "sub-race", &email_for(provider, "second"));
            let (a, b) = tokio::join!(
                find_or_create(&pool, &first),
                find_or_create(&pool, &second)
            );

            let a = a.expect("first login should succeed");
            let b = b.expect("second login should succeed");
            assert_eq!(a, b, "{provider:?}");
            assert_eq!(count_identities(&pool, provider).await, 1, "{provider:?}");
        }
        assert_eq!(count_users(&pool).await, 3);
    }

    #[sqlx::test]
    async fn logging_in_again_after_unlinking_links_the_same_account(pool: SqlitePool) {
        for provider in PROVIDERS {
            let email = email_for(provider, "relink");
            let user_id = insert_user(&pool, &email).await;
            let login = account(provider, "sub-1", &email);
            find_or_create(&pool, &login).await.expect("first login");
            assert!(
                matches!(
                    unlink(&pool, user_id, provider).await.expect("unlink"),
                    Unlinked::Done { .. }
                ),
                "{provider:?}"
            );

            let relinked = find_or_create(&pool, &login)
                .await
                .expect("login after unlinking");
            assert_eq!(relinked, user_id, "{provider:?}");
        }
    }

    /// 凍結中のアカウントは、紐付け済みの識別からも、同じメール (大文字小文字は問わない) からも見つける。
    #[sqlx::test]
    async fn frozen_target_finds_a_frozen_account_by_identity_or_email(pool: SqlitePool) {
        for provider in PROVIDERS {
            let linked_login = account(provider, "sub-linked", &email_for(provider, "linked"));
            let linked_id = find_or_create(&pool, &linked_login).await.expect("login");
            let unlinked_email = email_for(provider, "unlinked");
            let unlinked_id = insert_user(&pool, &unlinked_email).await;
            // 識別は持っているが、識別とは別のメールで来たログイン。
            let renamed_login = account(provider, "sub-linked", &email_for(provider, "renamed"));
            let unlinked_login = account(provider, "sub-new", &unlinked_email.to_uppercase());
            let new_login = account(provider, "sub-other", &email_for(provider, "new"));

            for login in [&linked_login, &renamed_login, &unlinked_login, &new_login] {
                assert_eq!(
                    frozen_target(&pool, login).await.expect("frozen_target"),
                    None,
                    "{login:?}"
                );
            }

            freeze(&pool, linked_id).await;
            freeze(&pool, unlinked_id).await;
            for (login, expected) in [
                (&linked_login, Some(linked_id)),
                (&renamed_login, Some(linked_id)),
                (&unlinked_login, Some(unlinked_id)),
                (&new_login, None),
            ] {
                assert_eq!(
                    frozen_target(&pool, login).await.expect("frozen_target"),
                    expected,
                    "{login:?}"
                );
            }
        }
    }

    /// Google・LINE のプロフィールは、ログインのたびに受け取った値で上書きする。
    #[sqlx::test]
    async fn save_profile_overwrites_the_profile_on_each_login(pool: SqlitePool) {
        // (名前, 画像, 保存される名前, 保存される画像)
        let cases = [
            // 名前の前後の空白は取り除く。
            (
                Some("  山田 花子  "),
                Some("https://lh3.googleusercontent.com/a/first"),
                Some("山田 花子"),
                Some("https://lh3.googleusercontent.com/a/first"),
            ),
            // https 以外の画像は保存しない。
            (
                Some("山田 はな"),
                Some("http://example.com/avatar.png"),
                Some("山田 はな"),
                None,
            ),
            // 空白だけの名前と、ホストの無い URL は NULL に戻す。
            (Some("   "), Some("https://"), None, None),
            (
                Some("山田 はな"),
                Some("https://example.com/a.png"),
                Some("山田 はな"),
                Some("https://example.com/a.png"),
            ),
            // 提供元で消された値も NULL に戻す。
            (None, None, None, None),
        ];
        for provider in [Provider::Google, Provider::Line] {
            let mut login = account(provider, "sub-1", &email_for(provider, "profile"));
            let user_id = find_or_create(&pool, &login).await.expect("login");
            for (name, picture, display_name, avatar_url) in cases {
                login.name = name.map(str::to_owned);
                login.picture = picture.map(str::to_owned);
                save_profile(&pool, user_id, &login)
                    .await
                    .expect("save_profile");
                let user = user_row(&pool, user_id).await;
                assert_eq!(
                    user.display_name.as_deref(),
                    display_name,
                    "{provider:?}: {name:?}"
                );
                assert_eq!(
                    user.avatar_url.as_deref(),
                    avatar_url,
                    "{provider:?}: {picture:?}"
                );
            }
        }
    }

    /// Apple は名前を最初の承認のときにしか渡さないので、渡ったときだけ入れ、無いときは残す。
    #[sqlx::test]
    async fn save_profile_of_apple_keeps_the_name_when_none_is_given(pool: SqlitePool) {
        let mut login = account(Provider::Apple, "sub-1", "apple@example.com");
        let user_id = find_or_create(&pool, &login).await.expect("login");

        login.name = Some("Taro Sato".to_owned());
        save_profile(&pool, user_id, &login)
            .await
            .expect("save_profile");
        login.name = None;
        save_profile(&pool, user_id, &login)
            .await
            .expect("save_profile");

        assert_eq!(
            user_row(&pool, user_id).await.display_name.as_deref(),
            Some("Taro Sato")
        );
    }

    /// 受け取らなかった方のトークンは前の値を残し、受け取った方は新しい値にする。
    #[sqlx::test]
    async fn save_revocation_tokens_keeps_the_token_not_received(pool: SqlitePool) {
        for provider in [Provider::Line, Provider::Apple] {
            // パスワードを持たせ、連携を外せるアカウントにする (外したときに返るトークンを見るため)。
            let email = email_for(provider, "tokens");
            let user_id = insert_user(&pool, &email).await;
            find_or_create(&pool, &account(provider, "sub-1", &email))
                .await
                .expect("login");
            for (refresh_token, access_token) in [
                (Some("refresh-1"), None),
                (None, Some("access-1")),
                (Some("refresh-2"), None),
            ] {
                let tokens = RevocationTokens {
                    refresh_token: refresh_token.map(str::to_owned),
                    access_token: access_token.map(str::to_owned),
                };
                save_revocation_tokens(&pool, provider, "sub-1", &tokens)
                    .await
                    .expect("tokens should be saved");
            }

            assert_eq!(
                unlink(&pool, user_id, provider).await.expect("unlink"),
                Unlinked::Done {
                    revocation_tokens: RevocationTokens {
                        refresh_token: Some("refresh-2".to_owned()),
                        access_token: Some("access-1".to_owned()),
                    }
                },
                "{provider:?}"
            );
        }
    }

    /// 外した後もログインできる方法 (パスワード・ほかの連携) が残るときだけ外せる。
    #[sqlx::test]
    async fn unlink_keeps_at_least_one_login_method(pool: SqlitePool) {
        // パスワードがあれば、唯一の連携も外せる。
        let with_password = insert_user(&pool, "password@example.com").await;
        find_or_create(
            &pool,
            &account(Provider::Google, "g-password", "password@example.com"),
        )
        .await
        .expect("login");
        assert_eq!(
            unlink(&pool, with_password, Provider::Google)
                .await
                .expect("unlink"),
            Unlinked::Done {
                revocation_tokens: RevocationTokens::default()
            }
        );
        assert_eq!(
            linked_providers(&pool, with_password)
                .await
                .expect("providers"),
            []
        );

        // パスワードが無ければ、連携が2つあるうちは片方を外せ、残った1つは外せない。
        let without_password =
            find_or_create(&pool, &account(Provider::Line, "l-1", "linked@example.com"))
                .await
                .expect("login");
        find_or_create(
            &pool,
            &account(Provider::Google, "g-1", "linked@example.com"),
        )
        .await
        .expect("login");
        assert!(matches!(
            unlink(&pool, without_password, Provider::Google)
                .await
                .expect("unlink"),
            Unlinked::Done { .. }
        ));
        assert_eq!(
            unlink(&pool, without_password, Provider::Line)
                .await
                .expect("unlink"),
            Unlinked::LastLoginMethod
        );
        assert_eq!(
            linked_providers(&pool, without_password)
                .await
                .expect("providers"),
            [Provider::Line]
        );
    }

    /// 連携していない提供元は外せない。他人の連携は、同じ提供元でも触れない。
    #[sqlx::test]
    async fn unlink_of_a_provider_that_is_not_linked_touches_nothing(pool: SqlitePool) {
        let user_id = insert_user(&pool, "kyoko@example.com").await;
        find_or_create(
            &pool,
            &account(Provider::Google, "other-sub", "other@example.com"),
        )
        .await
        .expect("login");

        for provider in PROVIDERS {
            assert_eq!(
                unlink(&pool, user_id, provider).await.expect("unlink"),
                Unlinked::NotLinked,
                "{provider:?}"
            );
        }
        assert_eq!(count_identities(&pool, Provider::Google).await, 1);
    }
}
