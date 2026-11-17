//! ログインの記録と知らせ (docs/authentication.md「ログインの記録と知らせ」)。決まりの中身は
//! `src/login_events.rs`・`src/login_notice.rs`・`src/login_alert.rs` の単体テストで確かめ、ここでは入口を通して
//! 記録・端末の印・メールがつながることを確かめる。

use super::*;

async fn auth_events(pool: &SqlitePool) -> Vec<(Option<i64>, String, String, String)> {
    sqlx::query!("SELECT user_id, kind, method, client FROM auth_events ORDER BY id")
        .fetch_all(pool)
        .await
        .expect("記録を読めるはず")
        .into_iter()
        .map(|row| (row.user_id, row.kind, row.method, row.client))
        .collect()
}

/// 失敗は応答を待たずに残すので、`count` 件残るまで待つ。余計に残る分も比べられるよう、その後も少し待つ。
async fn wait_for_events(pool: &SqlitePool, count: usize) {
    wait_until(async || (auth_events(pool).await.len() >= count).then_some(()))
        .await
        .unwrap_or_else(|| panic!("{count} 件の記録が残るはず"));
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
}

/// 同じブラウザ (端末の印の Cookie) からのログインは知らせず、別のブラウザからのログインは知らせる。
#[sqlx::test]
async fn login_notifies_only_from_a_new_device(pool: SqlitePool) {
    insert_user(&pool, "kyoko@example.com", "password").await;
    let user_id = user_id_of(&pool, "kyoko@example.com").await;
    let (app, sent) = test_app_with_mail(pool.clone()).await;

    let first = send(&app, login_request("kyoko@example.com", "password")).await;
    assert_eq!(first.status(), StatusCode::OK);
    let device = cookie_pair(&first, "device");
    let mails = wait_for_mails(&sent, 1).await;
    assert_eq!(mails[0].to, "kyoko@example.com");
    assert!(
        mails[0].subject.contains(LOGIN_NOTICE_SUBJECT),
        "{}",
        mails[0].subject
    );

    let same_browser = send(
        &app,
        login_request_with_cookie(&device, "kyoko@example.com", "password"),
    )
    .await;
    assert_eq!(same_browser.status(), StatusCode::OK);
    assert_mail_count_settles(&sent, 1).await;

    let other_browser = send(&app, login_request("kyoko@example.com", "password")).await;
    assert_eq!(other_browser.status(), StatusCode::OK);
    wait_for_mails(&sent, 2).await;

    let login = (
        Some(user_id),
        "login".to_string(),
        "password".to_string(),
        "web".to_string(),
    );
    assert_eq!(
        auth_events(&pool).await,
        vec![login.clone(), login.clone(), login]
    );
}

/// 失敗は残すが、相手を残すのは実在するアカウントのときだけ。制限に達した試行 (429) は残さない。
#[sqlx::test]
async fn login_failures_record_the_account_only_when_it_exists(pool: SqlitePool) {
    insert_user(&pool, "admin", "password").await;
    let user_id = user_id_of(&pool, "admin").await;
    let app = test_app(pool.clone()).await;

    let wrong_password = send(&app, login_request("admin", "wrong")).await;
    assert_eq!(wrong_password.status(), StatusCode::UNAUTHORIZED);
    let unknown_user = send(&app, login_request("someone@example.com", "password")).await;
    assert_eq!(unknown_user.status(), StatusCode::UNAUTHORIZED);

    wait_for_events(&pool, 2).await;
    let failed = |user_id| {
        (
            user_id,
            "login_failed".to_string(),
            "password".to_string(),
            "web".to_string(),
        )
    };
    assert_eq!(
        auth_events(&pool).await,
        vec![failed(Some(user_id)), failed(None)]
    );

    for _ in 0..10 {
        send(&app, login_request("admin", "wrong")).await;
    }
    // 1つのユーザー名へは1分に5回まで (docs/authentication.md「レートリミット」)。
    wait_for_events(&pool, 6).await;
    assert_eq!(auth_events(&pool).await.len(), 6);
}

/// 直近1時間の失敗が設定の件数に届いたら、運営者に1通だけ知らせる。
#[sqlx::test]
async fn login_failures_alert_the_operator(pool: SqlitePool) {
    insert_user(&pool, "admin", "password").await;
    let mut config = test_config();
    config.alerts.to = "ops@example.com".to_string();
    config.alerts.login_failures_per_hour = 3;
    let (app, sent) = test_app_with_mail_and_config(pool, &config).await;

    for _ in 0..2 {
        send(&app, login_request("admin", "wrong")).await;
    }
    assert_mail_count_settles(&sent, 0).await;

    for _ in 0..2 {
        send(&app, login_request("admin", "wrong")).await;
    }
    let mails = wait_for_mails(&sent, 1).await;
    assert_eq!(mails[0].to, "ops@example.com");
    assert_mail_count_settles(&sent, 1).await;
}

/// 送り先が空なら、運営者に知らせない。
#[sqlx::test]
async fn login_failures_do_not_alert_without_a_recipient(pool: SqlitePool) {
    insert_user(&pool, "admin", "password").await;
    let mut config = test_config();
    config.alerts.login_failures_per_hour = 1;
    let (app, sent) = test_app_with_mail_and_config(pool, &config).await;

    send(&app, login_request("admin", "wrong")).await;
    assert_mail_count_settles(&sent, 0).await;
}
