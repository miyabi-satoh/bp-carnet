//! パスワードの変更。

use super::*;

async fn count_sessions(pool: &SqlitePool) -> i64 {
    sqlx::query_scalar!(r#"SELECT COUNT(*) AS "count!: i64" FROM tower_sessions"#)
        .fetch_one(pool)
        .await
        .expect("failed to count sessions")
}

#[sqlx::test]
async fn change_password_lets_the_user_log_in_with_the_new_password(pool: SqlitePool) {
    let (app, cookie) = logged_in_app(pool).await;

    let response = send(
        &app,
        change_password_request(Some(&cookie), "password", "new-password"),
    )
    .await;
    assert_eq!(response.status(), StatusCode::NO_CONTENT);

    let old_password = send(&app, login_request("admin", "password")).await;
    assert_eq!(old_password.status(), StatusCode::UNAUTHORIZED);

    let new_password = send(&app, login_request("admin", "new-password")).await;
    assert_eq!(new_password.status(), StatusCode::OK);
}

/// パスワードを変えると、変更した端末以外のセッションは断たれる (docs/authentication.md)。
#[sqlx::test]
async fn change_password_cuts_off_sessions_on_other_devices(pool: SqlitePool) {
    insert_user(&pool, "admin", "password").await;
    let app = test_app(pool).await;
    // 2台の端末からログインした状態を作る。
    let other_device = login_and_get_cookie(&app, "admin", "password").await;
    let this_device = login_and_get_cookie(&app, "admin", "password").await;

    let response = send(
        &app,
        change_password_request(Some(&this_device), "password", "new-password"),
    )
    .await;
    assert_eq!(response.status(), StatusCode::NO_CONTENT);

    // 変更した端末はそのまま使える (操作した本人を追い出さない)。
    let same_device = send(&app, get("/api/v1/auth/me", Some(&this_device))).await;
    assert_eq!(same_device.status(), StatusCode::OK);

    let cut_off = send(&app, get("/api/v1/auth/me", Some(&other_device))).await;
    assert_eq!(cut_off.status(), StatusCode::UNAUTHORIZED);
}

/// 断たれたセッションは残留させない (次のアクセスで破棄する)。
#[sqlx::test]
async fn a_cut_off_session_is_flushed_from_the_store(pool: SqlitePool) {
    insert_user(&pool, "admin", "password").await;
    let app = test_app(pool.clone()).await;
    let other_device = login_and_get_cookie(&app, "admin", "password").await;
    let this_device = login_and_get_cookie(&app, "admin", "password").await;

    send(
        &app,
        change_password_request(Some(&this_device), "password", "new-password"),
    )
    .await;

    let before = count_sessions(&pool).await;
    send(&app, get("/api/v1/auth/me", Some(&other_device))).await;
    assert_eq!(
        count_sessions(&pool).await,
        before - 1,
        "断たれた側のセッションは破棄されるべき"
    );
}

/// 現在のパスワードの不一致は、未ログイン (401) と区別できるコードで返す。
#[sqlx::test]
async fn change_password_with_wrong_current_password_is_rejected(pool: SqlitePool) {
    let (app, cookie) = logged_in_app(pool).await;

    let response = send(
        &app,
        change_password_request(Some(&cookie), "wrong-password", "new-password"),
    )
    .await;

    assert_error(
        response,
        StatusCode::UNPROCESSABLE_ENTITY,
        "incorrect_password",
    )
    .await;

    let unchanged = send(&app, login_request("admin", "password")).await;
    assert_eq!(unchanged.status(), StatusCode::OK);
}

#[sqlx::test]
async fn change_password_rejects_a_password_below_the_configured_policy(pool: SqlitePool) {
    insert_user(&pool, "admin", "password").await;
    let config = strict_password_config();
    let app = test_app_with_config(pool, &config).await;
    let cookie = login_and_get_cookie(&app, "admin", "password").await;

    let response = send(
        &app,
        change_password_request(Some(&cookie), "password", "no-digits-here"),
    )
    .await;

    assert_error(
        response,
        StatusCode::UNPROCESSABLE_ENTITY,
        "validation_error",
    )
    .await;
}

/// Google 専用アカウントはパスワードを持たないため、設定画面の「パスワードを変更」を
/// 出さないための判定を `/auth/me` が返す。
#[sqlx::test]
async fn me_reports_whether_the_account_has_a_usable_password(pool: SqlitePool) {
    let (app, cookie) = logged_in_app(pool.clone()).await;

    let response = send(&app, get("/api/v1/auth/me", Some(&cookie))).await;
    assert_eq!(json_body(response).await["passwordUsable"], true);

    let mock = start_google_mock(google_claims("sub-1", "google@example.com", true)).await;
    let google_app = test_app_with_google_login(pool, &mock).await;
    let login = run_login_flow(&google_app, LoginProvider::Google).await;
    let google_cookie = session_cookie(&login);

    let response = send(&google_app, get("/api/v1/auth/me", Some(&google_cookie))).await;
    assert_eq!(json_body(response).await["passwordUsable"], false);
}

/// セッションを奪った相手に、現在のパスワードの総当たりや Argon2 の連続実行をさせないため、
/// 照合の失敗にも上限を掛ける。
#[sqlx::test]
async fn change_password_is_rate_limited_after_repeated_failures(pool: SqlitePool) {
    let (app, cookie) = logged_in_app(pool).await;

    let attempt = |current: &str| change_password_request(Some(&cookie), current, "new-password");

    // 上限 (5回/分) に達するまでは、単に現在のパスワードの誤りとして返る。
    for _ in 0..5 {
        let response = send(&app, attempt("wrong-password")).await;
        assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    }

    // 上限を超えると、正しいパスワードでも 429 になる。
    let limited = send(&app, attempt("password")).await;
    assert_error(limited, StatusCode::TOO_MANY_REQUESTS, "too_many_requests").await;

    // ログイン用の制限とは別の実体なので、ログインは巻き込まれない。
    let login = send(&app, login_request("admin", "password")).await;
    assert_eq!(login.status(), StatusCode::OK);
}

/// 変更に成功した分は上限の消費に数えないため、上限 (既定5回/分) を超える回数を続けて
/// 変更しても 429 にならない。
#[sqlx::test]
async fn successful_password_changes_do_not_consume_the_rate_limit(pool: SqlitePool) {
    let (app, cookie) = logged_in_app(pool).await;

    let mut current = "password".to_string();
    for round in 0..8 {
        let next = format!("password-{round}");
        let response = send(
            &app,
            change_password_request(Some(&cookie), &current, &next),
        )
        .await;
        assert_eq!(response.status(), StatusCode::NO_CONTENT);
        current = next;
    }
}

#[sqlx::test]
async fn password_policy_is_readable_without_logging_in(pool: SqlitePool) {
    let config = strict_password_config();
    let app = test_app_with_config(pool, &config).await;

    let response = send(&app, get("/api/v1/auth/password-policy", None)).await;

    assert_eq!(response.status(), StatusCode::OK);
    let body = json_body(response).await;
    assert_eq!(body["minLength"], 8);
    assert_eq!(body["requiredClasses"], json!(["digit"]));
}

const PASSWORD_CHANGED_SUBJECT: &str = "【BP Carnet】パスワードが変更されました";

/// `to` 宛てのパスワード変更のお知らせを待って返す。
async fn wait_for_password_notice(sent: &Arc<Mutex<Vec<SentMail>>>, to: &str) -> SentMail {
    wait_for_sent(sent, |mails| {
        mails
            .into_iter()
            .find(|mail| mail.to == to && mail.subject == PASSWORD_CHANGED_SUBJECT)
    })
    .await
    .unwrap_or_else(|| panic!("{to} 宛てにパスワード変更のお知らせが届くはず"))
}

#[sqlx::test]
async fn changing_the_password_notifies_the_user(pool: SqlitePool) {
    insert_user(&pool, "kyoko@example.com", "password").await;
    let (app, sent) = test_app_with_mail(pool).await;
    let cookie = login_and_get_cookie(&app, "kyoko@example.com", "password").await;

    let response = send(
        &app,
        change_password_request(Some(&cookie), "password", "new-password"),
    )
    .await;
    assert_eq!(response.status(), StatusCode::NO_CONTENT);

    let mail = wait_for_password_notice(&sent, "kyoko@example.com").await;
    assert!(mail.body.contains("設定画面からの変更"), "{}", mail.body);
    assert!(
        mail.body.contains("http://localhost:3010/reset-password\n"),
        "{}",
        mail.body
    );
}

#[sqlx::test]
async fn resetting_the_password_by_email_notifies_the_user(pool: SqlitePool) {
    insert_user(&pool, "kyoko@example.com", "password").await;
    let (app, sent) = test_app_with_mail(pool).await;

    let response = send(&app, password_reset_request("kyoko@example.com")).await;
    assert_eq!(response.status(), StatusCode::NO_CONTENT);
    let mails = wait_for_mails(&sent, 1).await;
    let response = send(
        &app,
        complete_password_reset_request(&link_token(&mails[0], "/reset-password"), "new-password"),
    )
    .await;
    assert_eq!(response.status(), StatusCode::NO_CONTENT);

    let mail = wait_for_password_notice(&sent, "kyoko@example.com").await;
    assert!(
        mail.body.contains("メールのリンクからの再設定"),
        "{}",
        mail.body
    );
}

#[sqlx::test]
async fn admin_resetting_the_password_notifies_the_user(pool: SqlitePool) {
    insert_admin(&pool, "admin", "password").await;
    insert_user(&pool, "kyoko@example.com", "password").await;
    let target_id = user_id_of(&pool, "kyoko@example.com").await;
    let (app, sent) = test_app_with_mail(pool).await;
    let cookie = login_and_get_cookie(&app, "admin", "password").await;

    let response = send(
        &app,
        reset_password_request(&cookie, target_id, "new-password"),
    )
    .await;
    assert_eq!(response.status(), StatusCode::NO_CONTENT);

    let mail = wait_for_password_notice(&sent, "kyoko@example.com").await;
    assert!(mail.body.contains("管理者による再設定"), "{}", mail.body);
}

#[sqlx::test]
async fn changing_the_password_does_not_mail_a_non_address_user_id(pool: SqlitePool) {
    insert_user(&pool, "kyoko", "password").await;
    let (app, sent) = test_app_with_mail(pool).await;
    let cookie = login_and_get_cookie(&app, "kyoko", "password").await;

    let response = send(
        &app,
        change_password_request(Some(&cookie), "password", "new-password"),
    )
    .await;
    assert_eq!(response.status(), StatusCode::NO_CONTENT);
    assert_mail_count_settles(&sent, 0).await;
}
