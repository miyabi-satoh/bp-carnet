//! ログインの失敗が増えたら、運営者にメールで知らせる (docs/authentication.md「ログインの記録と知らせ」)。
//!
//! 接続元を変えながら制限の内側で試すパスワードリスト攻撃は、レートリミットに達しないので、
//! 失敗の件数そのものを見る。

use std::sync::Mutex;
use std::time::{Duration, Instant};

use sqlx::SqlitePool;

use crate::config::AlertsConfig;
use crate::mail::Mailer;
use crate::state::AppState;

/// 知らせの間隔。攻撃の間じゅう送り続けて、メールの送信の枠を使い切らないため。
/// 数える範囲 (直近1時間) と揃える。
const INTERVAL: Duration = Duration::from_secs(60 * 60);

/// 本文に載せる接続元の数。多い順の上位で、攻撃の出どころの見当がつけば足りる。
const TOP_SOURCES: i64 = 5;

const SUBJECT: &str = "【BP Carnet】ログインの失敗が増えています";

/// 最後に知らせた時刻。プロセスの中だけで持つ (再起動で忘れても、1通多く送るだけで済む)。
#[derive(Debug, Default)]
pub struct LastSent(Mutex<Option<Instant>>);

impl LastSent {
    fn recently(&self, now: Instant) -> bool {
        self.0
            .lock()
            .expect("lock")
            .is_some_and(|sent| now.duration_since(sent) < INTERVAL)
    }

    /// 間隔を空けていれば今を記録して `true`。同時に届いた失敗から二重に送らないよう、確かめと記録を1回の鍵で行う。
    fn claim(&self, now: Instant) -> bool {
        let mut sent = self.0.lock().expect("lock");
        if sent.is_some_and(|sent| now.duration_since(sent) < INTERVAL) {
            return false;
        }
        *sent = Some(now);
        true
    }
}

/// ログインの失敗を記録した後に呼ぶ。応答を待たせないよう、別のタスクで確かめる。
pub fn spawn_check(state: &AppState) {
    if state.alerts.to.is_empty() || state.login_alert_sent.recently(Instant::now()) {
        return;
    }
    let state = state.clone();
    tokio::spawn(async move {
        if let Err(err) = check(
            &state.pool,
            &state.mailer,
            &state.alerts,
            &state.login_alert_sent,
        )
        .await
        {
            tracing::error!(error = %crate::error_chain_line(&err), "ログインの失敗の知らせを送れませんでした");
        }
    });
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("ログインの失敗を数えられません")]
    Db(#[from] sqlx::Error),
    #[error(transparent)]
    Mail(#[from] crate::mail::Error),
}

/// 直近1時間の失敗が閾値以上で、前の知らせから1時間空いていれば送る。送ったら `true`。
async fn check(
    pool: &SqlitePool,
    mailer: &Mailer,
    config: &AlertsConfig,
    last_sent: &LastSent,
) -> Result<bool, Error> {
    let summary = summarize(pool).await?;
    if summary.failures < i64::from(config.login_failures_per_hour)
        || !last_sent.claim(Instant::now())
    {
        return Ok(false);
    }
    tracing::warn!(
        failures = summary.failures,
        "ログインの失敗が増えたため、運営者に知らせます"
    );
    mailer.send(&config.to, SUBJECT, body(&summary)).await?;
    Ok(true)
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
         次の知らせは1時間後まで送りません。記録は auth_events の表にあります。\n",
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
    async fn check_sends_once_when_the_failures_reach_the_threshold(pool: SqlitePool) {
        let (mailer, sent) = Mailer::recording("https://bp.example.com");
        let last_sent = LastSent::default();

        fail(&pool, None, "192.0.2.1", 2).await;
        assert!(
            !check(&pool, &mailer, &config(3), &last_sent)
                .await
                .expect("確かめられる")
        );

        fail(&pool, None, "192.0.2.1", 1).await;
        assert!(
            check(&pool, &mailer, &config(3), &last_sent)
                .await
                .expect("確かめられる")
        );
        // 1時間に1通まで。
        fail(&pool, None, "192.0.2.1", 1).await;
        assert!(
            !check(&pool, &mailer, &config(3), &last_sent)
                .await
                .expect("確かめられる")
        );

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
