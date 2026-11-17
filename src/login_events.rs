//! ログインの成功と失敗の記録 (`auth_events`、docs/authentication.md「ログインの記録と知らせ」)。
//!
//! 不正なログインに本人と運営者が気づき、後から、いつ・どこから入られたかを調べられるようにするため。
//! 本人への知らせは [`crate::login_notice`]、運営者への知らせは [`crate::login_alert`]。

use std::net::IpAddr;
use std::sync::Arc;

use axum::http::{HeaderMap, HeaderValue};
use sqlx::SqlitePool;
use tokio::sync::mpsc;

use crate::config::AlertsConfig;
use crate::login_alert::Alerter;
use crate::mail::Mailer;

use crate::device_cookie::Device;
use crate::oauth_identity::Provider;
use crate::state::AppState;

/// ログインの方法。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Method {
    Password,
    Google,
    Line,
    Apple,
    /// サインアップの確定 (メールの確認リンクからパスワードを決めた)。
    Signup,
}

impl Method {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Password => "password",
            Self::Google => "google",
            Self::Line => "line",
            Self::Apple => "apple",
            Self::Signup => "signup",
        }
    }

    /// 本人への知らせの本文に書く名前。
    pub fn label(self) -> &'static str {
        match self {
            Self::Password => "メールアドレスとパスワード",
            Self::Google => "Google",
            Self::Line => "LINE",
            Self::Apple => "Apple",
            Self::Signup => "新規登録",
        }
    }
}

impl From<Provider> for Method {
    fn from(provider: Provider) -> Self {
        match provider {
            Provider::Google => Self::Google,
            Provider::Line => Self::Line,
            Provider::Apple => Self::Apple,
        }
    }
}

/// ログインした先。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Client {
    Web,
    /// モバイルアプリ (docs/mobile-app.md)。
    App,
}

impl Client {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Web => "web",
            Self::App => "app",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Web => "ブラウザー",
            Self::App => "アプリ",
        }
    }
}

/// 1回のログインの試み。記録の行に入れる、相手のアカウント以外の値。
#[derive(Debug, Clone, Copy)]
pub struct Attempt {
    pub method: Method,
    pub client: Client,
    /// 接続元。分からない構成では `None`。
    pub ip: Option<IpAddr>,
}

/// 記録を残す日数。GitHub のセキュリティログと同じ (ユーザーが決定)。プライバシーポリシーの保存する期間と揃える。
const RETENTION_DAYS: i64 = 90;

/// 成功を残す。`device_hash` はウェブだけ (`crate::device_cookie`)。
///
/// 呼び出し側は、これが失敗したらログインさせない (後から調べられないログインを作らないため)。
pub async fn record_success(
    pool: &SqlitePool,
    user_id: i64,
    attempt: Attempt,
    device_hash: Option<&str>,
) -> Result<(), sqlx::Error> {
    insert(pool, "login", Some(user_id), attempt, device_hash).await
}

/// ウェブのログインの成功を、ブラウザの端末の印 (`crate::device_cookie`) と一緒に残す。
/// 呼び出し側は、これが失敗したらログインさせない ([`record_success`] と同じ)。
pub async fn record_web_success(
    state: &AppState,
    user_id: i64,
    attempt: Attempt,
    headers: &HeaderMap,
) -> Result<WebLogin, sqlx::Error> {
    let device = Device::from_request(headers, state.secure_cookie);
    let hash = device.hash();
    let known = is_known_device(&state.pool, user_id, &hash).await?;
    record_success(&state.pool, user_id, attempt, Some(&hash)).await?;
    Ok(WebLogin {
        device_cookie: device.set_header(state.secure_cookie),
        new_device: !known,
        user_id,
        attempt,
    })
}

/// 残したウェブのログイン。ログインの証し (セッション) を渡した後に、印を置き、知らせる。
pub struct WebLogin {
    /// 端末の印を置く (置き直して期限を延ばす) `Set-Cookie` の値。
    pub device_cookie: HeaderValue,
    new_device: bool,
    user_id: i64,
    attempt: Attempt,
}

impl WebLogin {
    /// 新しい端末からのログインなら、本人に知らせる。サインアップの確定と、外部のログインでアカウントを
    /// 作ったときは呼ばない (本人が今作ったばかりのため)。
    pub fn notify_if_new_device(&self, state: &AppState) {
        if self.new_device {
            crate::login_notice::spawn(
                state,
                self.user_id,
                self.attempt.method,
                self.attempt.client,
            );
        }
    }
}

/// 失敗を残す。応答を待たずに作業役 ([`FailureRecorder`]) に渡す。失敗の応答を、記録の都合 (DB の書き込みの
/// 待ち・失敗) で変えないため。`user_id` は、実在するアカウントへの失敗のときだけ。
pub fn record_failure(state: &AppState, user_id: Option<i64>, attempt: Attempt) {
    state.login_failures.record(user_id, attempt);
}

/// 受けたまま残し終えていない失敗の上限。ログインの全体の上限 (1分300回、`AttemptRateLimiter::for_logins`)
/// の1分ぶん。DB が詰まっている間に、溜めた分でメモリを使い潰さないため。超えた分は残さずログに出す。
const FAILURE_QUEUE: usize = 300;

/// ログインの失敗を1つの作業役で残し、運営者に知らせるかを確かめる。失敗ごとにタスクを作ると、
/// 攻撃の間じゅう書き込みと集計が重なるため、溜まった分をまとめて残し、確かめはそのたびに1回にする。
#[derive(Debug, Clone)]
pub struct FailureRecorder(mpsc::Sender<Failure>);

#[derive(Debug)]
struct Failure {
    user_id: Option<i64>,
    attempt: Attempt,
}

impl FailureRecorder {
    /// 作業役を起動する。
    pub fn start(pool: SqlitePool, mailer: Arc<Mailer>, alerts: AlertsConfig) -> Self {
        let (sender, receiver) = mpsc::channel(FAILURE_QUEUE);
        tokio::spawn(run(receiver, pool, mailer, Alerter::new(alerts)));
        Self(sender)
    }

    fn record(&self, user_id: Option<i64>, attempt: Attempt) {
        if let Err(err) = self.0.try_send(Failure { user_id, attempt }) {
            tracing::warn!(error = %err, "ログインの失敗が溜まっているため、記録しません");
        }
    }
}

async fn run(
    mut receiver: mpsc::Receiver<Failure>,
    pool: SqlitePool,
    mailer: Arc<Mailer>,
    mut alerter: Alerter,
) {
    while let Some(first) = receiver.recv().await {
        let mut next = Some(first);
        // 失敗が届き続けても確かめに届くよう、一度に残すのはキューの上限ぶんまでにする。
        let mut remaining = FAILURE_QUEUE;
        while let Some(Failure { user_id, attempt }) = next {
            if let Err(err) = insert(&pool, "login_failed", user_id, attempt, None).await {
                tracing::error!(error = %crate::error_chain_line(&err), "ログインの失敗を記録できませんでした");
            }
            remaining -= 1;
            next = if remaining == 0 {
                None
            } else {
                receiver.try_recv().ok()
            };
        }
        if let Err(err) = alerter.check(&pool, &mailer).await {
            tracing::error!(error = %crate::error_chain_line(&err), "ログインの失敗の知らせを送れませんでした");
        }
    }
}

async fn insert(
    pool: &SqlitePool,
    kind: &str,
    user_id: Option<i64>,
    attempt: Attempt,
    device_hash: Option<&str>,
) -> Result<(), sqlx::Error> {
    let method = attempt.method.as_str();
    let client = attempt.client.as_str();
    let ip = attempt.ip.map(|ip| ip.to_canonical().to_string());
    sqlx::query!(
        "INSERT INTO auth_events (user_id, kind, method, client, ip, device_hash)
         VALUES (?, ?, ?, ?, ?, ?)",
        user_id,
        kind,
        method,
        client,
        ip,
        device_hash
    )
    .execute(pool)
    .await?;
    Ok(())
}

/// `device_hash` の端末から、このアカウントにログインしたことがあるか (残っている90日の記録の中で)。
pub async fn is_known_device(
    pool: &SqlitePool,
    user_id: i64,
    device_hash: &str,
) -> Result<bool, sqlx::Error> {
    sqlx::query_scalar!(
        r#"SELECT EXISTS (
               SELECT 1 FROM auth_events
               WHERE user_id = ? AND kind = 'login' AND device_hash = ?
           ) AS "known!: bool""#,
        user_id,
        device_hash
    )
    .fetch_one(pool)
    .await
}

/// 保存期間を過ぎた記録を消し、消した件数を返す。定期タスク (`crate::account::purge_expired_periodically`) から呼ぶ。
pub async fn purge_expired(pool: &SqlitePool) -> Result<u64, sqlx::Error> {
    let modifier = format!("-{RETENTION_DAYS} days");
    let result = sqlx::query!(
        "DELETE FROM auth_events
         WHERE created_at < strftime('%Y-%m-%dT%H:%M:%fZ', 'now', ?)",
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

    fn attempt(method: Method, client: Client) -> Attempt {
        Attempt {
            method,
            client,
            ip: Some("192.0.2.10".parse().expect("固定のアドレス")),
        }
    }

    #[sqlx::test]
    async fn a_device_is_known_only_after_a_success_on_the_same_account(pool: SqlitePool) {
        let alice = insert_user(&pool, "alice@example.com").await;
        let bob = insert_user(&pool, "bob@example.com").await;
        let web = attempt(Method::Password, Client::Web);

        assert!(
            !is_known_device(&pool, alice, "hash-1")
                .await
                .expect("読める")
        );
        record_success(&pool, alice, web, Some("hash-1"))
            .await
            .expect("残せる");
        assert!(
            is_known_device(&pool, alice, "hash-1")
                .await
                .expect("読める")
        );
        // 同じ端末でも、別のアカウントには初めてのログイン。
        assert!(!is_known_device(&pool, bob, "hash-1").await.expect("読める"));
    }

    /// 記録の行の値。IPv4 を写した IPv6 のアドレスは、IPv4 の形で残す。
    #[sqlx::test]
    async fn record_success_stores_the_attempt(pool: SqlitePool) {
        let alice = insert_user(&pool, "alice@example.com").await;
        let app = Attempt {
            method: Method::Google,
            client: Client::App,
            ip: Some("::ffff:192.0.2.10".parse().expect("固定のアドレス")),
        };
        record_success(&pool, alice, app, None)
            .await
            .expect("残せる");

        let row = sqlx::query!(
            r#"SELECT user_id, kind, method, client, ip, device_hash FROM auth_events"#
        )
        .fetch_one(&pool)
        .await
        .expect("1行あるはず");
        assert_eq!(row.user_id, Some(alice));
        assert_eq!(row.kind, "login");
        assert_eq!(row.method, "google");
        assert_eq!(row.client, "app");
        assert_eq!(row.ip.as_deref(), Some("192.0.2.10"));
        assert_eq!(row.device_hash, None);
    }

    #[sqlx::test]
    async fn purge_expired_deletes_only_rows_older_than_90_days(pool: SqlitePool) {
        let alice = insert_user(&pool, "alice@example.com").await;
        let web = attempt(Method::Password, Client::Web);
        record_success(&pool, alice, web, Some("old"))
            .await
            .expect("残せる");
        record_success(&pool, alice, web, Some("recent"))
            .await
            .expect("残せる");
        sqlx::query!(
            "UPDATE auth_events SET created_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now', '-91 days')
             WHERE device_hash = 'old'"
        )
        .execute(&pool)
        .await
        .expect("書き換えられる");
        sqlx::query!(
            "UPDATE auth_events SET created_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now', '-89 days')
             WHERE device_hash = 'recent'"
        )
        .execute(&pool)
        .await
        .expect("書き換えられる");

        assert_eq!(purge_expired(&pool).await.expect("消せる"), 1);
        assert!(!is_known_device(&pool, alice, "old").await.expect("読める"));
        assert!(
            is_known_device(&pool, alice, "recent")
                .await
                .expect("読める")
        );
    }

    #[sqlx::test]
    async fn deleting_the_account_deletes_its_records(pool: SqlitePool) {
        let alice = insert_user(&pool, "alice@example.com").await;
        record_success(&pool, alice, attempt(Method::Password, Client::Web), None)
            .await
            .expect("残せる");
        sqlx::query!("DELETE FROM users WHERE id = ?", alice)
            .execute(&pool)
            .await
            .expect("消せる");
        let count = sqlx::query_scalar!(r#"SELECT COUNT(*) AS "count!: i64" FROM auth_events"#)
            .fetch_one(&pool)
            .await
            .expect("数えられる");
        assert_eq!(count, 0);
    }
}
