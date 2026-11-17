//! ログインの失敗が増えたら、運営者にメールで知らせる (docs/authentication.md「ログインの記録と知らせ」)。
//!
//! 接続元を変えながら制限の内側で試すパスワードリスト攻撃は、レートリミットに達しないので、
//! 失敗の件数そのものを見る。

use std::time::{Duration, Instant};

use sqlx::SqlitePool;

use crate::config::AlertsConfig;
use crate::mail::Mailer;

/// 知らせの間隔。攻撃の間じゅう送り続けて、メールの送信の枠を使い切らないため。
/// 数える範囲 (直近1時間) と揃える。
const INTERVAL: Duration = Duration::from_secs(60 * 60);

/// 本文に載せる接続元の数。多い順の上位で、攻撃の出どころの見当がつけば足りる。
const TOP_SOURCES: i64 = 5;

const SUBJECT: &str = "【BP Carnet】ログインの失敗が増えています";
const ADMIN_CODE_SUBJECT: &str = "【BP Carnet】管理画面の確認コードが間違って入力されました";

/// 送れなかったときに、次に送り直すまでの間。失敗のたびに送り直して、メールの送信の枠や SMTP の接続を
/// 使い潰さないため。攻撃が続いていれば、この間の後の失敗でまた送る。
const RETRY_AFTER: Duration = Duration::from_secs(5 * 60);

/// 運営者への知らせ。ログインの失敗を残す作業役 (`crate::login_events::FailureRecorder`) だけが持つ
/// (知らせの間隔は、その作業役の中だけで持つ。再起動で忘れても、1通多く送るだけで済む)。
#[derive(Debug)]
pub struct Alerter {
    config: AlertsConfig,
    last_sent: Option<Instant>,
    retry_at: Option<Instant>,
    /// 確認コードの誤りの知らせを最後に送った時刻。ログインの失敗の知らせとは別に数える。
    admin_code_last_sent: Option<Instant>,
}

impl Alerter {
    pub fn new(config: AlertsConfig) -> Self {
        Self {
            config,
            last_sent: None,
            retry_at: None,
            admin_code_last_sent: None,
        }
    }

    /// 管理画面の確認コードの誤りを知らせる。管理者のパスワードが破られているおそれがあるので、
    /// 件数を待たずに送る。1時間に1通まで (照合の回数の制限の内で、誤りは1時間に300回まで起きうるため)。
    pub async fn admin_code_failed(
        &mut self,
        pool: &SqlitePool,
        mailer: &Mailer,
        user_id: i64,
        ip: Option<std::net::IpAddr>,
    ) -> Result<bool, Error> {
        let now = Instant::now();
        if self.config.to.is_empty()
            || self
                .admin_code_last_sent
                .is_some_and(|sent| now.duration_since(sent) < INTERVAL)
        {
            return Ok(false);
        }
        let username = crate::auth::load_username(pool, user_id)
            .await?
            .unwrap_or_default();
        let ip = ip.map_or_else(|| "不明".to_string(), |ip| ip.to_canonical().to_string());
        tracing::warn!(user_id, "管理画面の確認コードの誤りを、運営者に知らせます");
        mailer
            .send(
                &self.config.to,
                ADMIN_CODE_SUBJECT,
                format!(
                    "管理画面の確認コードが間違って入力されました。\n\
                     \n\
                     管理者: {username}\n\
                     接続元: {ip}\n\
                     \n\
                     入力した覚えが無い場合は、管理者のパスワードが知られているおそれがあります。パスワードを変えてください。\n\
                     この知らせは1時間に1通までです。\n"
                ),
            )
            .await?;
        self.admin_code_last_sent = Some(now);
        Ok(true)
    }

    /// 直近1時間の失敗が閾値以上で、前の知らせから1時間空いていれば送る。送ったら `true`。
    pub async fn check(&mut self, pool: &SqlitePool, mailer: &Mailer) -> Result<bool, Error> {
        let now = Instant::now();
        if self.config.to.is_empty()
            || self
                .last_sent
                .is_some_and(|sent| now.duration_since(sent) < INTERVAL)
            || self.retry_at.is_some_and(|retry_at| now < retry_at)
        {
            return Ok(false);
        }
        let failures = count_failures(pool).await?;
        if failures < i64::from(self.config.login_failures_per_hour) {
            return Ok(false);
        }
        tracing::warn!(failures, "ログインの失敗が増えたため、運営者に知らせます");
        let sent = async {
            let summary = summarize(pool).await?;
            mailer
                .send(&self.config.to, SUBJECT, body(&summary))
                .await?;
            Ok::<_, Error>(())
        }
        .await;
        match sent {
            Ok(()) => {
                self.last_sent = Some(now);
                self.retry_at = None;
                Ok(true)
            }
            Err(err) => {
                self.retry_at = Some(now + RETRY_AFTER);
                Err(err)
            }
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("ログインの失敗を数えられません")]
    Db(#[from] sqlx::Error),
    #[error(transparent)]
    Mail(#[from] crate::mail::Error),
}

async fn count_failures(pool: &SqlitePool) -> Result<i64, sqlx::Error> {
    sqlx::query_scalar!(
        r#"SELECT COUNT(*) AS "failures!: i64" FROM auth_events
           WHERE kind = 'login_failed'
             AND created_at >= strftime('%Y-%m-%dT%H:%M:%fZ', 'now', '-1 hour')"#
    )
    .fetch_one(pool)
    .await
}

/// 直近1時間の失敗のまとめ。
#[derive(Debug, PartialEq, Eq)]
struct Summary {
    failures: i64,
    /// うち実在するアカウントへの件数。
    known_account_failures: i64,
    accounts: i64,
    sources: i64,
    /// 多い順の接続元と件数。接続元が分からない構成の分は `None`。
    top_sources: Vec<(Option<String>, i64)>,
}

async fn summarize(pool: &SqlitePool) -> Result<Summary, sqlx::Error> {
    let counts = sqlx::query!(
        r#"SELECT COUNT(*) AS "failures!: i64",
                  COUNT(user_id) AS "known_account_failures!: i64",
                  COUNT(DISTINCT user_id) AS "accounts!: i64",
                  COUNT(DISTINCT ip) AS "sources!: i64"
           FROM auth_events
           WHERE kind = 'login_failed'
             AND created_at >= strftime('%Y-%m-%dT%H:%M:%fZ', 'now', '-1 hour')"#
    )
    .fetch_one(pool)
    .await?;
    let top_sources = sqlx::query!(
        r#"SELECT ip, COUNT(*) AS "count!: i64"
           FROM auth_events
           WHERE kind = 'login_failed'
             AND created_at >= strftime('%Y-%m-%dT%H:%M:%fZ', 'now', '-1 hour')
           GROUP BY ip
           ORDER BY 2 DESC, ip
           LIMIT ?"#,
        TOP_SOURCES
    )
    .fetch_all(pool)
    .await?
    .into_iter()
    .map(|row| (row.ip, row.count))
    .collect();
    Ok(Summary {
        failures: counts.failures,
        known_account_failures: counts.known_account_failures,
        accounts: counts.accounts,
        sources: counts.sources,
        top_sources,
    })
}

fn body(summary: &Summary) -> String {
    let top_sources = summary
        .top_sources
        .iter()
        .map(|(ip, count)| format!("- {}: {count}件", ip.as_deref().unwrap_or("不明")))
        .collect::<Vec<_>>()
        .join("\n");
    format!(
        "直近1時間のログインの失敗が {} 件ありました。\n\
         \n\
         うち実在するアカウントへの失敗: {} 件 ({} アカウント)\n\
         接続元の数: {}\n\
         \n\
         失敗の多い接続元:\n\
         {top_sources}\n\
         \n\
         相手のアカウントと接続元は、DB の auth_events の表で確かめられます。\n\
         この知らせは1時間に1通までです。\n",
        summary.failures, summary.known_account_failures, summary.accounts, summary.sources
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::insert_user;

    async fn fail(pool: &SqlitePool, user_id: Option<i64>, ip: &str, times: usize) {
        for _ in 0..times {
            sqlx::query!(
                "INSERT INTO auth_events (user_id, kind, method, client, ip)
                 VALUES (?, 'login_failed', 'password', 'web', ?)",
                user_id,
                ip
            )
            .execute(pool)
            .await
            .expect("残せる");
        }
    }

    fn config(threshold: u32) -> AlertsConfig {
        AlertsConfig {
            to: "ops@example.com".to_string(),
            login_failures_per_hour: threshold,
        }
    }

    #[sqlx::test]
    async fn summarize_counts_only_the_last_hour(pool: SqlitePool) {
        let alice = insert_user(&pool, "alice@example.com").await;
        let bob = insert_user(&pool, "bob@example.com").await;
        fail(&pool, Some(alice), "192.0.2.1", 3).await;
        fail(&pool, Some(bob), "192.0.2.2", 1).await;
        fail(&pool, None, "192.0.2.2", 1).await;
        fail(&pool, None, "192.0.2.3", 1).await;
        sqlx::query!(
            "UPDATE auth_events SET created_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now', '-61 minutes')
             WHERE ip = '192.0.2.3'"
        )
        .execute(&pool)
        .await
        .expect("書き換えられる");

        assert_eq!(
            summarize(&pool).await.expect("数えられる"),
            Summary {
                failures: 5,
                known_account_failures: 4,
                accounts: 2,
                sources: 2,
                top_sources: vec![
                    (Some("192.0.2.1".to_string()), 3),
                    (Some("192.0.2.2".to_string()), 2),
                ],
            }
        );
    }

    #[sqlx::test]
    async fn admin_code_failures_are_sent_once_an_hour(pool: SqlitePool) {
        let admin = insert_user(&pool, "admin@example.com").await;
        let (mailer, sent) = Mailer::recording("https://bp.example.com");
        let mut alerter = Alerter::new(config(20));
        let ip = Some("192.0.2.1".parse().expect("固定のアドレス"));

        assert!(
            alerter
                .admin_code_failed(&pool, &mailer, admin, ip)
                .await
                .expect("送れる")
        );
        assert!(
            !alerter
                .admin_code_failed(&pool, &mailer, admin, ip)
                .await
                .expect("送れる")
        );

        let sent = sent.lock().expect("lock");
        assert_eq!(sent.len(), 1);
        assert!(
            sent[0].body.contains("管理者: admin@example.com"),
            "{}",
            sent[0].body
        );
        assert!(
            sent[0].body.contains("接続元: 192.0.2.1"),
            "{}",
            sent[0].body
        );
    }

    #[sqlx::test]
    async fn check_sends_once_when_the_failures_reach_the_threshold(pool: SqlitePool) {
        let (mailer, sent) = Mailer::recording("https://bp.example.com");
        let mut alerter = Alerter::new(config(3));

        fail(&pool, None, "192.0.2.1", 2).await;
        assert!(!alerter.check(&pool, &mailer).await.expect("確かめられる"));

        fail(&pool, None, "192.0.2.1", 1).await;
        assert!(alerter.check(&pool, &mailer).await.expect("確かめられる"));
        // 1時間に1通まで。
        fail(&pool, None, "192.0.2.1", 1).await;
        assert!(!alerter.check(&pool, &mailer).await.expect("確かめられる"));

        let sent = sent.lock().expect("lock");
        assert_eq!(sent.len(), 1);
        assert_eq!(sent[0].to, "ops@example.com");
        assert!(sent[0].body.contains("3 件ありました"), "{}", sent[0].body);
        assert!(
            sent[0].body.contains("- 192.0.2.1: 3件"),
            "{}",
            sent[0].body
        );
    }
}
