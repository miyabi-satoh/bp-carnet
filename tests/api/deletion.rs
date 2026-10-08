//! 退会の予約と取り消し。

use super::*;

fn schedule_deletion_request(cookie: Option<&str>) -> Request<Body> {
    post_json("/api/v1/account/deletion", cookie, json!({}))
}

#[sqlx::test]
async fn schedule_deletion_returns_the_scheduled_date(pool: SqlitePool) {
    let (app, cookie) = logged_in_app(pool).await;

    let response = send(&app, schedule_deletion_request(Some(&cookie))).await;

    assert_eq!(response.status(), StatusCode::OK);
    let body = json_body(response).await;
    assert_eq!(body["graceDays"], 30);
    assert!(
        body["deletionScheduledAt"]
            .as_str()
            .is_some_and(|value| value.ends_with('Z')),
        "削除予定日時が UTC の文字列で返るはず: {body}"
    );
}

/// 設定画面に予定日を出すため、`/auth/me` が予約中の削除の予定日と起点を返す。
#[sqlx::test]
async fn me_reports_a_scheduled_deletion(pool: SqlitePool) {
    let (app, cookie) = logged_in_app(pool).await;
    let before = json_body(send(&app, get("/api/v1/auth/me", Some(&cookie))).await).await;
    assert_eq!(before["deletionScheduledAt"], Value::Null);
    assert_eq!(before["deletionOrigin"], Value::Null);

    let scheduled = json_body(send(&app, schedule_deletion_request(Some(&cookie))).await).await;
    let after = json_body(send(&app, get("/api/v1/auth/me", Some(&cookie))).await).await;

    assert_eq!(
        after["deletionScheduledAt"],
        scheduled["deletionScheduledAt"]
    );
    assert_eq!(after["deletionOrigin"], "user");
}

/// 猶予期間中のログインは予約の取り消しを兼ねる (専用の取り消し UI は持たない)。
#[sqlx::test]
async fn logging_in_cancels_a_scheduled_deletion(pool: SqlitePool) {
    let (app, cookie) = logged_in_app(pool.clone()).await;
    let scheduled = send(&app, schedule_deletion_request(Some(&cookie))).await;
    assert_eq!(scheduled.status(), StatusCode::OK);

    let response = send(&app, login_request("admin", "password")).await;

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(json_body(response).await["deletionCancelled"], true);
    assert_eq!(deletion_scheduled_at_of(&pool, "admin").await, None);
}

#[sqlx::test]
async fn logging_in_without_a_scheduled_deletion_reports_no_cancellation(pool: SqlitePool) {
    insert_user(&pool, "admin", "password").await;
    let app = test_app(pool).await;

    let response = send(&app, login_request("admin", "password")).await;

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(json_body(response).await["deletionCancelled"], false);
}

/// 自動退会の予約は、ログインでも、セッションを使った操作でも取り消される (docs/authentication.md)。
#[sqlx::test]
async fn an_inactivity_deletion_is_cancelled_by_login_and_by_using_the_session(pool: SqlitePool) {
    let (app, cookie) = logged_in_app(pool.clone()).await;
    let schedule = || async {
        sqlx::query!(
            "UPDATE users SET deletion_scheduled_at = '2030-01-01T00:00:00.000Z', deletion_origin = 'inactive'"
        )
        .execute(&pool)
        .await
        .expect("自動退会の予約を書けるはず");
    };

    schedule().await;
    let response = send(&app, login_request("admin", "password")).await;
    assert_eq!(json_body(response).await["deletionCancelled"], true);
    assert_eq!(deletion_scheduled_at_of(&pool, "admin").await, None);

    schedule().await;
    let response = send(&app, get("/api/v1/records", Some(&cookie))).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(deletion_scheduled_at_of(&pool, "admin").await, None);
}

/// 本人が削除を予約すると、予定日と取り消し方法をメールで知らせる。押し直しでは送り直さない。
#[sqlx::test]
async fn schedule_deletion_mails_the_user_only_once(pool: SqlitePool) {
    insert_user(&pool, "kyoko@example.com", "password").await;
    // 既定 (Asia/Tokyo) ではなく本人のタイムゾーンで書くことを確かめるため、既定と変えておく。
    sqlx::query!(
        "UPDATE users SET timezone = 'Pacific/Honolulu' WHERE username = 'kyoko@example.com'"
    )
    .execute(&pool)
    .await
    .expect("タイムゾーンを変えられるはず");
    let (app, sent) = test_app_with_mail(pool).await;
    let cookie = login_and_get_cookie(&app, "kyoko@example.com", "password").await;

    let mut scheduled_at = None;
    for _ in 0..2 {
        let response = send(&app, schedule_deletion_request(Some(&cookie))).await;
        assert_eq!(response.status(), StatusCode::OK);
        scheduled_at = json_body(response).await["deletionScheduledAt"]
            .as_str()
            .map(str::to_string);
    }

    let expected_date = scheduled_at
        .expect("削除予定日時が返るはず")
        .parse::<jiff::Timestamp>()
        .expect("RFC3339 のはず")
        .in_tz("Pacific/Honolulu")
        .expect("既知のタイムゾーンのはず")
        .strftime("%Y年%-m月%-d日以降に")
        .to_string();
    let mails = wait_for_mails(&sent, 1).await;
    assert_eq!(mails[0].to, "kyoko@example.com");
    assert!(
        mails[0].body.contains(&expected_date)
            && mails[0].body.contains("http://localhost:3010/login"),
        "予定日 ({expected_date}) とログインのリンクが載るはず: {}",
        mails[0].body
    );
    assert_mail_count_settles(&sent, 1).await;
}

/// ユーザーIDがメールアドレスでなければ送り先が無いので送らない。
#[sqlx::test]
async fn schedule_deletion_does_not_mail_a_non_address_user_id(pool: SqlitePool) {
    insert_user(&pool, "kyoko", "password").await;
    let (app, sent) = test_app_with_mail(pool).await;
    let cookie = login_and_get_cookie(&app, "kyoko", "password").await;

    let response = send(&app, schedule_deletion_request(Some(&cookie))).await;

    assert_eq!(response.status(), StatusCode::OK);
    assert_mail_count_settles(&sent, 0).await;
}

/// 管理者が起点の削除では本人に知らせない (docs/authentication.md)。
#[sqlx::test]
async fn admin_delete_does_not_mail_the_user(pool: SqlitePool) {
    insert_admin(&pool, "admin@example.com", "password").await;
    insert_user(&pool, "kyoko@example.com", "password").await;
    let target_id = user_id_of(&pool, "kyoko@example.com").await;
    let (app, sent) = test_app_with_mail(pool).await;
    let cookie = login_and_get_cookie(&app, "admin@example.com", "password").await;

    let response = send(&app, admin_delete_request(&cookie, target_id)).await;

    assert_eq!(response.status(), StatusCode::OK);
    assert_mail_count_settles(&sent, 0).await;
}

/// Google ログインでも同じく取り消される (ボディを返せないため遷移先で知らせる)。
#[sqlx::test]
async fn google_callback_cancels_a_scheduled_deletion(pool: SqlitePool) {
    let mock = start_google_mock(google_claims("sub-1", "scheduled@example.com", true)).await;
    let app = test_app_with_google_login(pool.clone(), &mock).await;
    let first = run_login_flow(&app, LoginProvider::Google).await;
    assert_eq!(location_header(&first), "/");
    set_deletion_scheduled(
        &pool,
        "scheduled@example.com",
        "2100-01-01T00:00:00.000Z",
        "user",
    )
    .await;

    let response = run_login_flow(&app, LoginProvider::Google).await;

    assert_eq!(response.status(), StatusCode::SEE_OTHER);
    assert_eq!(location_header(&response), "/?deletionCancelled=1");
    assert_eq!(
        deletion_scheduled_at_of(&pool, "scheduled@example.com").await,
        None
    );
}
