//! モバイルアプリのログインのトークン (docs/mobile-app.md)。
//!
//! アプリの画面は `capacitor://localhost` から出るため、`SameSite=Strict` のセッション Cookie が
//! 送られない。代わりにログインで渡したトークンを `Authorization: Bearer` で受け取る。
//! DB にはハッシュだけを持ち、失効の条件はセッションにそろえる (最終利用からの日数・世代・凍結・削除)。

use axum::http::{HeaderMap, header};
use sqlx::SqlitePool;

use crate::token;

/// アプリの画面の出どころ (Capacitor の iOS の既定)。CORS で許し、同一オリジンチェックから外す
/// (`crate::csrf`)。
pub const APP_ORIGIN: &str = "capacitor://localhost";

/// アプリの画面からのリクエストか (`Origin` で見分ける)。偽れるので、表示を変えることと、アプリに更新を促すか
/// (`crate::app_version`。外れても困るのは古い版のアプリだけ) にだけ使う。
pub fn is_from_app(headers: &HeaderMap) -> bool {
    headers
        .get(header::ORIGIN)
        .is_some_and(|origin| origin.as_bytes() == APP_ORIGIN.as_bytes())
}

/// `Authorization: Bearer <token>` のトークン。他の方式や読めない値は、無いものとして扱う
/// (その場合はセッションで判定する)。
pub fn bearer(headers: &HeaderMap) -> Option<&str> {
    let value = headers.get(header::AUTHORIZATION)?.to_str().ok()?;
    let (scheme, token) = value.split_once(' ')?;
    let token = token.trim();
    (scheme.eq_ignore_ascii_case("bearer") && !token.is_empty()).then_some(token)
}

/// ログインが決まった利用者にトークンを渡す (ウェブの `auth::establish_session` に当たる)。
pub async fn establish(pool: &SqlitePool, user_id: i64) -> Result<String, sqlx::Error> {
    // ログインも、使われていることの記録に数える (docs/authentication.md)。
    crate::account::touch_last_seen(pool, user_id).await?;
    issue(pool, user_id).await
}

/// トークンを発行する。今の `users.session_generation` を持たせる。
pub async fn issue(pool: &SqlitePool, user_id: i64) -> Result<String, sqlx::Error> {
    let token = token::random_url_safe(32);
    let token_hash = token::sha256_url_safe(&token);
    sqlx::query!(
        "INSERT INTO app_tokens (user_id, token_hash, session_generation)
         SELECT id, ?, session_generation FROM users WHERE id = ?",
        token_hash,
        user_id
    )
    .execute(pool)
    .await?;
    Ok(token)
}

/// 有効なトークンの持ち主。
#[derive(Debug, PartialEq, Eq)]
pub struct Found {
    pub id: i64,
    pub user_id: i64,
    pub session_generation: i64,
    /// 最終利用の記録が1日以上前か。読むだけのリクエストで毎回書かないよう、古いときだけ更新する。
    pub needs_touch: bool,
}

/// 最終利用から `expiry_days` 日以内のトークンを引く。世代・凍結は呼び出し側が確かめる。
pub async fn find(
    pool: &SqlitePool,
    token: &str,
    expiry_days: i64,
) -> Result<Option<Found>, sqlx::Error> {
    let token_hash = token::sha256_url_safe(token);
    let modifier = format!("-{expiry_days} days");
    sqlx::query_as!(
        Found,
        r#"SELECT id AS "id!: i64", user_id, session_generation,
                  (last_used_at < strftime('%Y-%m-%dT%H:%M:%fZ', 'now', '-1 day')) AS "needs_touch!: bool"
           FROM app_tokens
           WHERE token_hash = ?
             AND last_used_at >= strftime('%Y-%m-%dT%H:%M:%fZ', 'now', ?)"#,
        token_hash,
        modifier
    )
    .fetch_optional(pool)
    .await
}

pub async fn touch(pool: &SqlitePool, id: i64) -> Result<(), sqlx::Error> {
    sqlx::query!(
        "UPDATE app_tokens SET last_used_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now') WHERE id = ?",
        id
    )
    .execute(pool)
    .await?;
    Ok(())
}

/// 無効と分かったトークンを消す。読んだときの世代のままのときだけ消す: パスワードを変えた端末の
/// トークンは、読んだ後に新しい世代へ載せ替えられていることがあり、それは消さない (→ `auth::update_password_hash`)。
pub async fn discard(
    pool: &SqlitePool,
    id: i64,
    session_generation: i64,
) -> Result<(), sqlx::Error> {
    sqlx::query!(
        "DELETE FROM app_tokens WHERE id = ? AND session_generation = ?",
        id,
        session_generation
    )
    .execute(pool)
    .await?;
    Ok(())
}

/// トークンそのものを無効にする (ログアウト)。無いトークンでも成功にする。
pub async fn revoke(pool: &SqlitePool, token: &str) -> Result<(), sqlx::Error> {
    let token_hash = token::sha256_url_safe(token);
    sqlx::query!("DELETE FROM app_tokens WHERE token_hash = ?", token_hash)
        .execute(pool)
        .await?;
    Ok(())
}

/// 失効したトークン (最終利用から `expiry_days` 日を過ぎたもの・世代がずれたもの) を消す。
/// 失効の判定は [`find`] と呼び出し側で済むので、これは溜まらないための掃除。
pub async fn purge_expired(pool: &SqlitePool, expiry_days: i64) -> Result<u64, sqlx::Error> {
    let modifier = format!("-{expiry_days} days");
    let result = sqlx::query!(
        "DELETE FROM app_tokens
         WHERE last_used_at < strftime('%Y-%m-%dT%H:%M:%fZ', 'now', ?)
            OR session_generation <> (SELECT session_generation FROM users WHERE users.id = app_tokens.user_id)",
        modifier
    )
    .execute(pool)
    .await?;
    Ok(result.rows_affected())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::insert_user;
    use axum::http::HeaderValue;

    fn headers(value: &str) -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert(
            header::AUTHORIZATION,
            HeaderValue::from_str(value).expect("ヘッダーの値にできるはず"),
        );
        headers
    }

    #[test]
    fn bearer_reads_only_the_bearer_scheme() {
        assert_eq!(bearer(&headers("Bearer abc")), Some("abc"));
        assert_eq!(bearer(&headers("bearer abc")), Some("abc"));
        assert_eq!(bearer(&headers("Basic abc")), None);
        assert_eq!(bearer(&headers("Bearer ")), None);
        assert_eq!(bearer(&HeaderMap::new()), None);
    }

    #[sqlx::test]
    async fn issued_token_is_found_until_it_expires(pool: SqlitePool) {
        let user_id = insert_user(&pool, "alice").await;
        let token = issue(&pool, user_id)
            .await
            .expect("テストの DB 操作は成功するはず");

        let found = find(&pool, &token, 30)
            .await
            .expect("テストの DB 操作は成功するはず")
            .expect("見つかるはず");
        assert_eq!(found.user_id, user_id);
        assert!(!found.needs_touch);
        assert_eq!(
            find(&pool, "other", 30)
                .await
                .expect("テストの DB 操作は成功するはず"),
            None
        );

        sqlx::query!(
            "UPDATE app_tokens SET last_used_at = '2000-01-01T00:00:00.000Z' WHERE id = ?",
            found.id
        )
        .execute(&pool)
        .await
        .expect("テストの DB 操作は成功するはず");
        assert_eq!(
            find(&pool, &token, 30)
                .await
                .expect("テストの DB 操作は成功するはず"),
            None,
            "30日を過ぎたら無効"
        );
    }

    #[sqlx::test]
    async fn stored_value_is_only_the_hash(pool: SqlitePool) {
        let user_id = insert_user(&pool, "alice").await;
        let token = issue(&pool, user_id)
            .await
            .expect("テストの DB 操作は成功するはず");
        let stored = sqlx::query_scalar!("SELECT token_hash FROM app_tokens")
            .fetch_one(&pool)
            .await
            .expect("テストの DB 操作は成功するはず");
        assert_ne!(stored, token);
    }

    #[sqlx::test]
    async fn discard_keeps_a_token_moved_to_a_newer_generation(pool: SqlitePool) {
        let user_id = insert_user(&pool, "alice").await;
        let token = issue(&pool, user_id).await.expect("発行できるはず");
        let read = find(&pool, &token, 30)
            .await
            .expect("引けるはず")
            .expect("見つかるはず");

        // 読んだ後に、パスワード変更で新しい世代へ載せ替えられた。
        sqlx::query!(
            "UPDATE app_tokens SET session_generation = session_generation + 1 WHERE id = ?",
            read.id
        )
        .execute(&pool)
        .await
        .expect("更新できるはず");
        discard(&pool, read.id, read.session_generation)
            .await
            .expect("消す操作は成功するはず");
        assert!(find(&pool, &token, 30).await.expect("引けるはず").is_some());

        discard(&pool, read.id, read.session_generation + 1)
            .await
            .expect("消す操作は成功するはず");
        assert!(find(&pool, &token, 30).await.expect("引けるはず").is_none());
    }

    #[sqlx::test]
    async fn purge_removes_expired_and_outdated_tokens(pool: SqlitePool) {
        let user_id = insert_user(&pool, "alice").await;
        let live = issue(&pool, user_id)
            .await
            .expect("テストの DB 操作は成功するはず");
        let old = issue(&pool, user_id)
            .await
            .expect("テストの DB 操作は成功するはず");
        let old_id = find(&pool, &old, 30)
            .await
            .expect("テストの DB 操作は成功するはず")
            .expect("テストの DB 操作は成功するはず")
            .id;
        sqlx::query!(
            "UPDATE app_tokens SET last_used_at = '2000-01-01T00:00:00.000Z' WHERE id = ?",
            old_id
        )
        .execute(&pool)
        .await
        .expect("テストの DB 操作は成功するはず");

        let other = insert_user(&pool, "bob").await;
        issue(&pool, other)
            .await
            .expect("テストの DB 操作は成功するはず");
        sqlx::query!(
            "UPDATE users SET session_generation = session_generation + 1 WHERE id = ?",
            other
        )
        .execute(&pool)
        .await
        .expect("テストの DB 操作は成功するはず");

        assert_eq!(
            purge_expired(&pool, 30)
                .await
                .expect("テストの DB 操作は成功するはず"),
            2
        );
        assert!(
            find(&pool, &live, 30)
                .await
                .expect("テストの DB 操作は成功するはず")
                .is_some()
        );
    }
}
