//! 管理者の API と凍結。

use super::*;

/// 管理者 `admin` と操作の対象がいて、管理者でログインした状態。
struct AdminSession {
    app: Router,
    cookie: String,
    admin_id: i64,
    target_id: i64,
}

/// 管理者 `admin` と、`role` の権限の対象 `target` を作り、管理者でログインする。
async fn admin_session(pool: &SqlitePool, target: &str, role: auth::Role) -> AdminSession {
    admin_session_with_config(pool, target, role, &test_config()).await
}

async fn admin_session_with_config(
    pool: &SqlitePool,
    target: &str,
    role: auth::Role,
    config: &Config,
) -> AdminSession {
    insert_admin(pool, "admin", "password").await;
    insert_user_with_role(pool, target, "password", role).await;
    let app = test_app_with_config(pool.clone(), config).await;
    let cookie = login_and_get_cookie(&app, "admin", "password").await;
    AdminSession {
        admin_id: user_id_of(pool, "admin").await,
        target_id: user_id_of(pool, target).await,
        app,
        cookie,
    }
}

/// 一般ユーザー `kyoko` を対象にした [`admin_session`]。
async fn admin_and_kyoko(pool: &SqlitePool) -> AdminSession {
    admin_session(pool, "kyoko", auth::Role::User).await
}

/// 操作する管理者 `admin` の削除を予約し、有効な管理者を0人にする (削除を予約済みの管理者は数えない)。
async fn leave_no_active_admin(pool: &SqlitePool) {
    set_deletion_scheduled(pool, "admin", "2100-01-01T00:00:00.000Z", "user").await;
}

fn set_ocr_limit_request(cookie: &str, target_id: i64, budget: Value) -> Request<Body> {
    put_json(
        &format!("/api/v1/admin/users/{target_id}/ocr-limit"),
        Some(cookie),
        json!({ "ocrBudgetYen": budget }),
    )
}

#[sqlx::test]
async fn admin_users_lists_every_user_in_creation_order(pool: SqlitePool) {
    let s = admin_and_kyoko(&pool).await;

    let response = send(&s.app, get("/api/v1/admin/users", Some(&s.cookie))).await;

    assert_eq!(response.status(), StatusCode::OK);
    let body = json_body(response).await;
    let users = body["users"].as_array().expect("users は配列のはず");
    assert_eq!(users.len(), 2);
    assert_eq!(users[0]["username"], "admin");
    assert_eq!(users[0]["role"], "admin");
    assert_eq!(users[1]["username"], "kyoko");
    assert_eq!(users[1]["role"], "user");
    // 凍結は既定のまま。
    assert_eq!(users[1]["frozen"], false);
}

/// 一覧は、凍結・OCR上限といった管理に要る状態をそのまま返す。
#[sqlx::test]
async fn admin_users_reports_per_user_state(pool: SqlitePool) {
    let s = admin_and_kyoko(&pool).await;
    freeze_user(&pool, "kyoko").await;
    set_deletion_scheduled(&pool, "kyoko", "2030-01-01T00:00:00.000Z", "admin").await;

    let response = send(&s.app, get("/api/v1/admin/users", Some(&s.cookie))).await;

    let body = json_body(response).await;
    let kyoko = &body["users"][1];
    assert_eq!(kyoko["frozen"], true);
    assert_eq!(kyoko["deletionScheduledAt"], "2030-01-01T00:00:00.000Z");
    assert_eq!(kyoko["deletionOrigin"], "admin");
    assert_eq!(body["users"][0]["deletionOrigin"], Value::Null);
}

/// `passwordUsable` は一覧の「パスワード再設定...」を出すかの判断に使われるため、
/// Google 専用アカウントと ID/PW アカウントで値が分かれることを固定する。
#[sqlx::test]
async fn admin_users_reports_whether_a_user_has_a_password(pool: SqlitePool) {
    insert_admin(&pool, "admin", "password").await;
    let mock = start_google_mock(google_claims("sub-1", "google@example.com", true)).await;
    let app = test_app_with_google_login(pool, &mock).await;
    run_login_flow(&app, LoginProvider::Google).await;
    let cookie = login_and_get_cookie(&app, "admin", "password").await;

    let response = send(&app, get("/api/v1/admin/users", Some(&cookie))).await;

    let body = json_body(response).await;
    let users = body["users"].as_array().expect("users should be an array");
    assert_eq!(users[0]["username"], "admin");
    assert_eq!(users[0]["passwordUsable"], true);
    assert_eq!(users[1]["username"], "google@example.com");
    assert_eq!(users[1]["passwordUsable"], false);
}

#[sqlx::test]
async fn admin_user_detail_returns_account_with_an_audit_entry(pool: SqlitePool) {
    let s = admin_and_kyoko(&pool).await;
    sqlx::query!(
        "UPDATE users SET timezone = 'America/New_York', morning_start_min = 300 WHERE username = 'kyoko'"
    )
    .execute(&pool)
    .await
    .expect("テストデータを更新できるはず");

    let response = send(
        &s.app,
        get(
            &format!("/api/v1/admin/users/{}", s.target_id),
            Some(&s.cookie),
        ),
    )
    .await;

    assert_eq!(response.status(), StatusCode::OK);
    let body = json_body(response).await;
    assert_eq!(body["username"], "kyoko");
    assert_eq!(body["passwordUsable"], true);
    assert_eq!(body["googleLinked"], false);
    assert_eq!(body["lineLinked"], false);
    assert_eq!(body["timezone"], "America/New_York");
    assert_eq!(body["morningStartMin"], 300);
    assert!(body["createdAt"].is_string());
    assert_eq!(
        audit_log_entries(&pool).await,
        vec![(s.admin_id, s.target_id, "view_account".to_string())]
    );
}

/// 期間は対象ユーザーのタイムゾーンで区切り、管理者自身の記録は混ざらない。
#[sqlx::test]
async fn admin_user_records_returns_target_records_in_target_timezone(pool: SqlitePool) {
    let s = admin_and_kyoko(&pool).await;
    // 既定の Asia/Tokyo (UTC+9) では、UTC 2026-09-06T23:00 は 9月7日 8:00 (朝)。
    insert_record_at(&pool, "kyoko", "2026-09-06T23:00:00Z", 131).await;
    insert_record_at(&pool, "kyoko", "2026-09-08T23:00:00Z", 140).await;
    insert_record_at(&pool, "admin", "2026-09-06T23:00:00Z", 150).await;

    let response = send(
        &s.app,
        get(
            &format!(
                "/api/v1/admin/users/{}/records?from=2026-09-07&to=2026-09-07",
                s.target_id
            ),
            Some(&s.cookie),
        ),
    )
    .await;

    assert_eq!(response.status(), StatusCode::OK);
    let body = json_body(response).await;
    let records = body["records"].as_array().expect("records は配列のはず");
    assert_eq!(records.len(), 1);
    assert_eq!(records[0]["systolic"], 131);
    assert_eq!(records[0]["localMeasuredAt"], "2026-09-07T08:00");
    assert_eq!(records[0]["dayPeriod"], "morning");
    assert_eq!(body["summary"]["morning"]["systolic"], 131.0);
}

/// 取得のたびに1件記録する。一覧と集計を1回の取得にまとめているので、1回で2件にはならない。
#[sqlx::test]
async fn admin_user_records_adds_an_audit_entry_per_fetch(pool: SqlitePool) {
    let s = admin_and_kyoko(&pool).await;

    for _ in 0..2 {
        let response = send(
            &s.app,
            get(
                &format!("/api/v1/admin/users/{}/records", s.target_id),
                Some(&s.cookie),
            ),
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);
    }

    assert_eq!(
        audit_log_entries(&pool).await,
        vec![
            (s.admin_id, s.target_id, "view_records".to_string()),
            (s.admin_id, s.target_id, "view_records".to_string()),
        ]
    );
}

/// 期間の指定が不正なら、記録を返さず監査ログにも残さない。
#[sqlx::test]
async fn admin_user_records_rejects_invalid_range_without_an_audit_entry(pool: SqlitePool) {
    let s = admin_and_kyoko(&pool).await;

    let response = send(
        &s.app,
        get(
            &format!(
                "/api/v1/admin/users/{}/records?from=2026-09-08&to=2026-09-07",
                s.target_id
            ),
            Some(&s.cookie),
        ),
    )
    .await;

    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    assert!(audit_log_entries(&pool).await.is_empty());
}

#[sqlx::test]
async fn set_frozen_freezes_and_unfreezes_with_an_audit_entry(pool: SqlitePool) {
    let s = admin_and_kyoko(&pool).await;

    let freeze = send(&s.app, set_frozen_request(&s.cookie, s.target_id, true)).await;
    assert_eq!(freeze.status(), StatusCode::NO_CONTENT);
    assert!(is_frozen(&pool, "kyoko").await);

    let unfreeze = send(&s.app, set_frozen_request(&s.cookie, s.target_id, false)).await;
    assert_eq!(unfreeze.status(), StatusCode::NO_CONTENT);
    assert!(!is_frozen(&pool, "kyoko").await);

    assert_eq!(
        audit_log_entries(&pool).await,
        vec![
            (s.admin_id, s.target_id, "freeze".to_string()),
            (s.admin_id, s.target_id, "unfreeze".to_string()),
        ]
    );
}

/// 自分を凍結すると、その場で管理画面から締め出される。
#[sqlx::test]
async fn set_frozen_rejects_freezing_yourself(pool: SqlitePool) {
    let s = admin_session(&pool, "other-admin", auth::Role::Admin).await;

    let response = send(&s.app, set_frozen_request(&s.cookie, s.admin_id, true)).await;

    assert_error(response, StatusCode::CONFLICT, "cannot_freeze_user").await;
    assert!(!is_frozen(&pool, "admin").await);
    assert!(audit_log_entries(&pool).await.is_empty());
}

/// 凍結された管理者は、その時点で管理APIを呼べなくなる。単独のリクエストで「有効な管理者が
/// 残らない」状態を作れないのは、これが効いているため。
#[sqlx::test]
async fn frozen_admin_cannot_use_admin_api(pool: SqlitePool) {
    let s = admin_and_kyoko(&pool).await;
    freeze_user(&pool, "admin").await;

    let response = send(&s.app, set_frozen_request(&s.cookie, s.target_id, true)).await;

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    assert!(!is_frozen(&pool, "kyoko").await);
}

/// 2人の管理者が互いを同時に凍結しても、有効な管理者が必ず1人は残る。
/// 実装の残数チェック (判定と更新を同じトランザクションに入れる) が守る性質。
#[sqlx::test]
async fn concurrent_freezes_keep_an_active_admin(pool: SqlitePool) {
    insert_admin(&pool, "admin-a", "password").await;
    insert_admin(&pool, "admin-b", "password").await;
    let id_a = user_id_of(&pool, "admin-a").await;
    let id_b = user_id_of(&pool, "admin-b").await;
    let app = test_app(pool.clone()).await;
    let cookie_a = login_and_get_cookie(&app, "admin-a", "password").await;
    let cookie_b = login_and_get_cookie(&app, "admin-b", "password").await;

    // a が b を、b が a を同時に凍結しにいく。
    tokio::join!(
        send(&app, set_frozen_request(&cookie_a, id_b, true)),
        send(&app, set_frozen_request(&cookie_b, id_a, true)),
    );

    let active_admins = sqlx::query_scalar!(
        r#"SELECT COUNT(*) AS "count!: i64" FROM users WHERE role = 'admin' AND frozen = 0"#
    )
    .fetch_one(&pool)
    .await
    .expect("管理者を数えられるはず");
    assert_eq!(active_admins, 1, "有効な管理者が残らなくなってはいけない");
}

#[sqlx::test]
async fn set_ocr_limit_sets_the_budget_and_the_list_shows_it(pool: SqlitePool) {
    let s = admin_and_kyoko(&pool).await;
    sqlx::query!("UPDATE users SET ocr_spent_milli_yen = 1500 WHERE username = 'kyoko'")
        .execute(&pool)
        .await
        .expect("failed to set the spent amount");

    let set = send(
        &s.app,
        set_ocr_limit_request(&s.cookie, s.target_id, json!(80)),
    )
    .await;
    assert_eq!(set.status(), StatusCode::NO_CONTENT);

    let list = send(&s.app, get("/api/v1/admin/users", Some(&s.cookie))).await;
    let body = json_body(list).await;
    let kyoko = body["users"]
        .as_array()
        .expect("users should be an array")
        .iter()
        .find(|user| user["username"] == "kyoko")
        .expect("kyoko should be listed");
    assert_eq!(kyoko["ocrBudgetYen"], 80);
    // 1.5円は切り上げて2円と出す。
    assert_eq!(kyoko["ocrSpentYen"], 2);
    // 設定の既定は無制限。
    assert_eq!(body["ocrDefaultBudgetYen"], -1);
}

/// 上限は `null` (既定に従う)・`-1` (無制限)・0〜100,000 円。受け付けた変更は1件ずつ監査ログに残し、
/// 値域の外は保存も記録もしない。
#[sqlx::test]
async fn set_ocr_limit_accepts_only_the_allowed_budgets(pool: SqlitePool) {
    let s = admin_and_kyoko(&pool).await;

    for (budget, accepted) in [(json!(80), true), (json!(-2), false), (json!(null), true)] {
        let before = ocr_budget_yen_of(&pool, "kyoko").await;
        let response = send(
            &s.app,
            set_ocr_limit_request(&s.cookie, s.target_id, budget.clone()),
        )
        .await;
        if accepted {
            assert_eq!(response.status(), StatusCode::NO_CONTENT, "{budget}");
            assert_eq!(
                ocr_budget_yen_of(&pool, "kyoko").await,
                budget.as_i64(),
                "{budget}"
            );
        } else {
            assert_eq!(
                response.status(),
                StatusCode::UNPROCESSABLE_ENTITY,
                "{budget}"
            );
            assert_eq!(ocr_budget_yen_of(&pool, "kyoko").await, before, "{budget}");
        }
    }

    assert_eq!(
        audit_log_entries(&pool).await,
        vec![(s.admin_id, s.target_id, "change_ocr_limit".to_string()); 2]
    );
}

#[sqlx::test]
async fn admin_create_user_creates_a_login_ready_user_with_an_audit_entry(pool: SqlitePool) {
    insert_admin(&pool, "admin", "password").await;
    let admin_id = user_id_of(&pool, "admin").await;
    let app = test_app(pool.clone()).await;
    let cookie = login_and_get_cookie(&app, "admin", "password").await;

    let response = send(
        &app,
        create_user_request(
            &cookie,
            json!({"username": "Kyoko", "password": "password"}),
        ),
    )
    .await;

    assert_eq!(response.status(), StatusCode::CREATED);
    let body = json_body(response).await;
    // 保存される形 (小文字) を返す。
    assert_eq!(body["username"], "kyoko");
    let created_id = body["id"].as_i64().expect("id は数値のはず");
    assert_eq!(created_id, user_id_of(&pool, "kyoko").await);

    // 作ったユーザーでそのままログインできる (メール確認済みで作られる)。
    let login = send(&app, login_request("kyoko", "password")).await;
    assert_eq!(login.status(), StatusCode::OK);

    assert_eq!(
        audit_log_entries(&pool).await,
        vec![(admin_id, created_id, "create".to_string())]
    );
}

#[sqlx::test]
async fn admin_create_user_can_create_an_admin(pool: SqlitePool) {
    insert_admin(&pool, "admin", "password").await;
    let app = test_app(pool.clone()).await;
    let cookie = login_and_get_cookie(&app, "admin", "password").await;

    let response = send(
        &app,
        create_user_request(
            &cookie,
            json!({"username": "second", "password": "password", "role": "admin"}),
        ),
    )
    .await;

    assert_eq!(response.status(), StatusCode::CREATED);
    let role = sqlx::query_scalar!(r#"SELECT role FROM users WHERE username = 'second'"#)
        .fetch_one(&pool)
        .await
        .expect("作ったユーザーを読めるはず");
    assert_eq!(role, "admin");
}

/// 大文字違いも同じユーザーIDとして扱う (保存前に小文字へ正規化するため)。
#[sqlx::test]
async fn admin_create_user_rejects_a_taken_username(pool: SqlitePool) {
    let s = admin_and_kyoko(&pool).await;

    let response = send(
        &s.app,
        create_user_request(
            &s.cookie,
            json!({"username": "KYOKO", "password": "password"}),
        ),
    )
    .await;

    assert_error(response, StatusCode::CONFLICT, "username_taken").await;
    assert_eq!(count_users(&pool).await, 2);
    assert!(audit_log_entries(&pool).await.is_empty());
}

/// 使えない文字の場合ごとの判定は `validation::validate_username` の単体テストで確かめる。
#[sqlx::test]
async fn admin_create_user_rejects_an_invalid_username(pool: SqlitePool) {
    insert_admin(&pool, "admin", "password").await;
    let app = test_app(pool.clone()).await;
    let cookie = login_and_get_cookie(&app, "admin", "password").await;

    let response = send(
        &app,
        create_user_request(
            &cookie,
            json!({"username": "with space", "password": "password"}),
        ),
    )
    .await;

    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(count_users(&pool).await, 1);
    assert!(audit_log_entries(&pool).await.is_empty());
}

/// パスワードは設定のポリシーで判定する (本人によるパスワード変更と同じ規則)。
#[sqlx::test]
async fn admin_create_user_rejects_a_weak_password(pool: SqlitePool) {
    insert_admin(&pool, "admin", "password").await;
    let app = test_app(pool.clone()).await;
    let cookie = login_and_get_cookie(&app, "admin", "password").await;

    let response = send(
        &app,
        create_user_request(&cookie, json!({"username": "newbie", "password": "x"})),
    )
    .await;

    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(count_users(&pool).await, 1);
}

#[sqlx::test]
async fn reset_password_lets_the_user_log_in_with_the_new_password(pool: SqlitePool) {
    let s = admin_and_kyoko(&pool).await;

    let response = send(
        &s.app,
        reset_password_request(&s.cookie, s.target_id, "new-password"),
    )
    .await;
    assert_eq!(response.status(), StatusCode::NO_CONTENT);

    let old_password = send(&s.app, login_request("kyoko", "password")).await;
    assert_eq!(old_password.status(), StatusCode::UNAUTHORIZED);

    let new_password = send(&s.app, login_request("kyoko", "new-password")).await;
    assert_eq!(new_password.status(), StatusCode::OK);

    assert_eq!(
        audit_log_entries(&pool).await,
        vec![(s.admin_id, s.target_id, "reset_password".to_string())]
    );
}

/// 再設定は対象のセッションをすべて断つ (docs/authentication.md)。操作しているのは別人なので、
/// 本人によるパスワード変更と違い生き残らせるセッションが無い。
#[sqlx::test]
async fn reset_password_cuts_off_every_session_of_the_target(pool: SqlitePool) {
    let s = admin_and_kyoko(&pool).await;
    let first_device = login_and_get_cookie(&s.app, "kyoko", "password").await;
    let second_device = login_and_get_cookie(&s.app, "kyoko", "password").await;

    send(
        &s.app,
        reset_password_request(&s.cookie, s.target_id, "new-password"),
    )
    .await;

    for (label, cookie) in [("1台目", &first_device), ("2台目", &second_device)] {
        let response = send(&s.app, get("/api/v1/auth/me", Some(cookie))).await;
        assert_eq!(
            response.status(),
            StatusCode::UNAUTHORIZED,
            "{label}のセッションも断たれるべき"
        );
    }

    // 操作した管理者のセッションは無関係なので続く。
    let admin_session = send(&s.app, get("/api/v1/auth/me", Some(&s.cookie))).await;
    assert_eq!(admin_session.status(), StatusCode::OK);
}

/// パスワードを持たないユーザー (Google 専用アカウント) には設定しない。新しく持たせるのは
/// 本人のメール確認が前提で、管理者が起点の再設定とは別の機能になる。
#[sqlx::test]
async fn reset_password_rejects_a_google_only_account(pool: SqlitePool) {
    insert_admin(&pool, "admin", "password").await;
    let mock = start_google_mock(google_claims("sub-1", "google@example.com", true)).await;
    let app = test_app_with_google_login(pool.clone(), &mock).await;
    run_login_flow(&app, LoginProvider::Google).await;
    let target_id = user_id_of(&pool, "google@example.com").await;
    let cookie = login_and_get_cookie(&app, "admin", "password").await;

    let response = send(
        &app,
        reset_password_request(&cookie, target_id, "new-password"),
    )
    .await;

    assert_error(response, StatusCode::CONFLICT, "cannot_reset_password").await;
    assert_eq!(session_generation_of(&pool, "google@example.com").await, 0);
    assert!(audit_log_entries(&pool).await.is_empty());
}

/// 自分自身は管理APIから再設定できない。本人の変更 (`PUT /account/password`) は現在の
/// パスワードの照合を通すため、こちらを迂回路にさせない。
#[sqlx::test]
async fn reset_password_rejects_the_admin_themselves(pool: SqlitePool) {
    insert_admin(&pool, "admin", "password").await;
    let admin_id = user_id_of(&pool, "admin").await;
    let app = test_app(pool.clone()).await;
    let cookie = login_and_get_cookie(&app, "admin", "password").await;

    let response = send(
        &app,
        reset_password_request(&cookie, admin_id, "new-password"),
    )
    .await;

    assert_error(response, StatusCode::CONFLICT, "cannot_reset_own_password").await;
    let still_logged_in = send(&app, get("/api/v1/auth/me", Some(&cookie))).await;
    assert_eq!(still_logged_in.status(), StatusCode::OK);
    let login = send(&app, login_request("admin", "password")).await;
    assert_eq!(login.status(), StatusCode::OK);
    assert_eq!(session_generation_of(&pool, "admin").await, 0);
    assert!(audit_log_entries(&pool).await.is_empty());
}

#[sqlx::test]
async fn reset_password_rejects_a_password_below_the_configured_policy(pool: SqlitePool) {
    let s = admin_session_with_config(&pool, "kyoko", auth::Role::User, &strict_password_config())
        .await;

    let response = send(
        &s.app,
        reset_password_request(&s.cookie, s.target_id, "no-digits-here"),
    )
    .await;

    assert_error(
        response,
        StatusCode::UNPROCESSABLE_ENTITY,
        "validation_error",
    )
    .await;
    let unchanged = send(&s.app, login_request("kyoko", "password")).await;
    assert_eq!(unchanged.status(), StatusCode::OK);
    assert_eq!(session_generation_of(&pool, "kyoko").await, 0);
    assert!(audit_log_entries(&pool).await.is_empty());
}

#[sqlx::test]
async fn admin_delete_schedules_with_an_audit_entry(pool: SqlitePool) {
    let s = admin_and_kyoko(&pool).await;

    let response = send(&s.app, admin_delete_request(&s.cookie, s.target_id)).await;

    assert_eq!(response.status(), StatusCode::OK);
    let body = json_body(response).await;
    assert_eq!(body["graceDays"], 30);
    assert_eq!(
        deletion_scheduled_at_of(&pool, "kyoko").await.as_deref(),
        body["deletionScheduledAt"].as_str()
    );
    assert_eq!(
        audit_log_entries(&pool).await,
        vec![(s.admin_id, s.target_id, "delete".to_string())]
    );
}

/// 押し直しても予定日は動かない (本人による削除と同じフロー)。何も変わらなくても記録は残す。
#[sqlx::test]
async fn admin_delete_keeps_the_first_schedule(pool: SqlitePool) {
    let s = admin_and_kyoko(&pool).await;
    set_deletion_scheduled(&pool, "kyoko", "2030-01-01T00:00:00.000Z", "admin").await;

    let response = send(&s.app, admin_delete_request(&s.cookie, s.target_id)).await;

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        json_body(response).await["deletionScheduledAt"],
        "2030-01-01T00:00:00.000Z"
    );
    assert_eq!(
        audit_log_entries(&pool).await,
        vec![(s.admin_id, s.target_id, "delete".to_string())]
    );
}

/// 管理者が起点の予約は、本人がログインしても取り消されない (凍結されていなくても)。
/// 起点ごとの取り消しの判定は `account::cancel_user_deletion` の単体テストで、ほかのログインの経路
/// (凍結中・外部のログイン) が同じ関数を通ることは、本人が起点の予約を取り消すテストで確かめる。
#[sqlx::test]
async fn logging_in_keeps_a_deletion_scheduled_by_an_admin(pool: SqlitePool) {
    let s = admin_and_kyoko(&pool).await;
    let scheduled = send(&s.app, admin_delete_request(&s.cookie, s.target_id)).await;
    assert_eq!(scheduled.status(), StatusCode::OK);

    let response = send(&s.app, login_request("kyoko", "password")).await;

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(json_body(response).await["deletionCancelled"], false);
    assert!(deletion_scheduled_at_of(&pool, "kyoko").await.is_some());
    let origin = sqlx::query_scalar!(
        r#"SELECT deletion_origin AS "deletion_origin: String" FROM users WHERE username = 'kyoko'"#
    )
    .fetch_one(&pool)
    .await
    .expect("ユーザーを読めるはず");
    assert_eq!(origin.as_deref(), Some("admin"));
}

/// 有効な管理者が0人でも、予約済みの管理者への再削除は数を変えないので受け付けて記録する。
#[sqlx::test]
async fn admin_delete_accepts_an_admin_already_pending_deletion_without_an_active_admin(
    pool: SqlitePool,
) {
    let s = admin_session(&pool, "other-admin", auth::Role::Admin).await;
    set_deletion_scheduled(&pool, "other-admin", "2100-01-01T00:00:00.000Z", "admin").await;
    leave_no_active_admin(&pool).await;

    let response = send(&s.app, admin_delete_request(&s.cookie, s.target_id)).await;

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        audit_log_entries(&pool).await,
        vec![(s.admin_id, s.target_id, "delete".to_string())]
    );
}

/// 凍結も同じ。有効な管理者が0人でも、凍結済みの管理者への再凍結は受け付けて記録する。
#[sqlx::test]
async fn set_frozen_accepts_an_already_frozen_admin_without_an_active_admin(pool: SqlitePool) {
    let s = admin_session(&pool, "other-admin", auth::Role::Admin).await;
    freeze_user(&pool, "other-admin").await;
    leave_no_active_admin(&pool).await;

    let response = send(&s.app, set_frozen_request(&s.cookie, s.target_id, true)).await;

    assert_eq!(response.status(), StatusCode::NO_CONTENT);
    assert_eq!(
        audit_log_entries(&pool).await,
        vec![(s.admin_id, s.target_id, "freeze".to_string())]
    );
}

/// 同じ状態への凍結も受け付けて記録する (何も変わらなかった操作も記録する)。
#[sqlx::test]
async fn set_frozen_records_freezing_an_already_frozen_user(pool: SqlitePool) {
    let s = admin_and_kyoko(&pool).await;
    freeze_user(&pool, "kyoko").await;

    let response = send(&s.app, set_frozen_request(&s.cookie, s.target_id, true)).await;

    assert_eq!(response.status(), StatusCode::NO_CONTENT);
    assert_eq!(
        audit_log_entries(&pool).await,
        vec![(s.admin_id, s.target_id, "freeze".to_string())]
    );
}

#[sqlx::test]
async fn admin_cancel_deletion_clears_a_schedule_by_an_admin_with_an_audit_entry(pool: SqlitePool) {
    let s = admin_and_kyoko(&pool).await;
    set_deletion_scheduled(&pool, "kyoko", "2100-01-01T00:00:00.000Z", "admin").await;

    let response = send(
        &s.app,
        admin_cancel_deletion_request(&s.cookie, s.target_id),
    )
    .await;

    assert_eq!(response.status(), StatusCode::NO_CONTENT);
    assert_eq!(deletion_scheduled_at_of(&pool, "kyoko").await, None);
    assert_eq!(
        audit_log_entries(&pool).await,
        vec![(s.admin_id, s.target_id, "cancel_deletion".to_string())]
    );
}

/// 本人の削除の申し出は、管理者からは覆さない (本人はログインすれば取り消せる)。自動退会の予約も
/// 同じ扱いになることは `account` の単体テスト `inactivity_deletion_is_cancelled_by_the_user_but_not_by_an_admin` で確かめる。
#[sqlx::test]
async fn admin_cancel_deletion_keeps_a_schedule_by_the_user(pool: SqlitePool) {
    let s = admin_and_kyoko(&pool).await;
    set_deletion_scheduled(&pool, "kyoko", "2100-01-01T00:00:00.000Z", "user").await;

    let response = send(
        &s.app,
        admin_cancel_deletion_request(&s.cookie, s.target_id),
    )
    .await;

    assert_error(response, StatusCode::CONFLICT, "cannot_cancel_deletion").await;
    assert!(deletion_scheduled_at_of(&pool, "kyoko").await.is_some());
    assert!(audit_log_entries(&pool).await.is_empty());
}

/// 予約が無くても受け付け、記録は残す (何も変わらなかった操作も記録する)。
#[sqlx::test]
async fn admin_cancel_deletion_without_a_schedule_is_recorded(pool: SqlitePool) {
    let s = admin_and_kyoko(&pool).await;

    let response = send(
        &s.app,
        admin_cancel_deletion_request(&s.cookie, s.target_id),
    )
    .await;

    assert_eq!(response.status(), StatusCode::NO_CONTENT);
    assert_eq!(
        audit_log_entries(&pool).await,
        vec![(s.admin_id, s.target_id, "cancel_deletion".to_string())]
    );
}

/// 管理画面から自分を消せてしまうのは事故のもと (本人の削除は設定画面にある)。
#[sqlx::test]
async fn admin_delete_rejects_yourself(pool: SqlitePool) {
    insert_admin(&pool, "admin", "password").await;
    let admin_id = user_id_of(&pool, "admin").await;
    let app = test_app(pool.clone()).await;
    let cookie = login_and_get_cookie(&app, "admin", "password").await;

    let response = send(&app, admin_delete_request(&cookie, admin_id)).await;

    assert_error(response, StatusCode::CONFLICT, "cannot_delete_user").await;
    assert_eq!(deletion_scheduled_at_of(&pool, "admin").await, None);
    assert!(audit_log_entries(&pool).await.is_empty());
}

/// 削除を予約済みの管理者は「有効な管理者」に数えないため、自分以外を弾くだけでは
/// 有効な管理者が0人になりうる。
#[sqlx::test]
async fn admin_delete_keeps_an_active_admin(pool: SqlitePool) {
    let s = admin_session(&pool, "other-admin", auth::Role::Admin).await;
    leave_no_active_admin(&pool).await;

    let response = send(&s.app, admin_delete_request(&s.cookie, s.target_id)).await;

    assert_eq!(response.status(), StatusCode::CONFLICT);
    assert_eq!(deletion_scheduled_at_of(&pool, "other-admin").await, None);
    assert!(audit_log_entries(&pool).await.is_empty());
}

/// 凍結も同じ。操作する管理者自身が削除待ちなら、相手を凍結すると残るのは削除待ちの1人だけになる。
#[sqlx::test]
async fn set_frozen_ignores_admins_pending_deletion(pool: SqlitePool) {
    let s = admin_session(&pool, "other-admin", auth::Role::Admin).await;
    leave_no_active_admin(&pool).await;

    let response = send(&s.app, set_frozen_request(&s.cookie, s.target_id, true)).await;

    assert_eq!(response.status(), StatusCode::CONFLICT);
    assert!(!is_frozen(&pool, "other-admin").await);
    assert!(audit_log_entries(&pool).await.is_empty());
}

/// 有効な管理者の残数を確かめるのは、対象が管理者のときだけ。自分が削除予約済みで有効な
/// 管理者が0人でも、一般ユーザーの削除は管理者の数を変えないため通す。
#[sqlx::test]
async fn admin_delete_allows_deleting_a_regular_user_without_an_active_admin(pool: SqlitePool) {
    let s = admin_and_kyoko(&pool).await;
    leave_no_active_admin(&pool).await;

    let response = send(&s.app, admin_delete_request(&s.cookie, s.target_id)).await;

    assert_eq!(response.status(), StatusCode::OK);
    assert!(deletion_scheduled_at_of(&pool, "kyoko").await.is_some());
}

/// 凍結も同じ (対象が一般ユーザーなら、有効な管理者の数は変わらない)。
#[sqlx::test]
async fn set_frozen_allows_freezing_a_regular_user_without_an_active_admin(pool: SqlitePool) {
    let s = admin_and_kyoko(&pool).await;
    leave_no_active_admin(&pool).await;

    let response = send(&s.app, set_frozen_request(&s.cookie, s.target_id, true)).await;

    assert_eq!(response.status(), StatusCode::NO_CONTENT);
    assert!(is_frozen(&pool, "kyoko").await);
}

/// 2人の管理者が互いを同時に削除しても、有効な管理者が必ず1人は残る。
#[sqlx::test]
async fn concurrent_deletions_keep_an_active_admin(pool: SqlitePool) {
    insert_admin(&pool, "admin-a", "password").await;
    insert_admin(&pool, "admin-b", "password").await;
    let id_a = user_id_of(&pool, "admin-a").await;
    let id_b = user_id_of(&pool, "admin-b").await;
    let app = test_app(pool.clone()).await;
    let cookie_a = login_and_get_cookie(&app, "admin-a", "password").await;
    let cookie_b = login_and_get_cookie(&app, "admin-b", "password").await;

    tokio::join!(
        send(&app, admin_delete_request(&cookie_a, id_b)),
        send(&app, admin_delete_request(&cookie_b, id_a)),
    );

    let active_admins = sqlx::query_scalar!(
        r#"SELECT COUNT(*) AS "count!: i64" FROM users
           WHERE role = 'admin' AND frozen = 0 AND deletion_scheduled_at IS NULL"#
    )
    .fetch_one(&pool)
    .await
    .expect("管理者を数えられるはず");
    assert_eq!(active_admins, 1, "有効な管理者が残らなくなってはいけない");
}

/// 凍結中はログインできない。パスワードの照合を通ってから弾くため、間違ったパスワードとは
/// 区別できるコードを返す。
#[sqlx::test]
async fn frozen_user_cannot_log_in(pool: SqlitePool) {
    insert_user(&pool, "kyoko", "password").await;
    freeze_user(&pool, "kyoko").await;
    let app = test_app(pool).await;

    let response = send(&app, login_request("kyoko", "password")).await;
    assert_error(response, StatusCode::FORBIDDEN, "account_frozen").await;

    // パスワードが違えば、凍結の有無を伝えずに 401 のまま。
    let wrong = send(&app, login_request("kyoko", "wrong-password")).await;
    assert_error(wrong, StatusCode::UNAUTHORIZED, "invalid_credentials").await;
}

/// ログイン中に凍結されたら、セッションの継続も断つ。
#[sqlx::test]
async fn freezing_a_user_ends_their_session(pool: SqlitePool) {
    insert_user(&pool, "kyoko", "password").await;
    let app = test_app(pool.clone()).await;
    let cookie = login_and_get_cookie(&app, "kyoko", "password").await;
    freeze_user(&pool, "kyoko").await;

    let response = send(&app, get("/api/v1/auth/me", Some(&cookie))).await;

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

/// 拒否する Google ログインで、`oauth_identities` に恒久的な紐付けを作らない
/// (紐付けは一度きりでやり直せないため)。未連携のメールアドレスの一致だけでは本人と判断できないので、
/// 本人が起点の削除予約も取り消さない。
#[sqlx::test]
async fn google_callback_does_not_link_a_frozen_account(pool: SqlitePool) {
    insert_user(&pool, "frozen@example.com", "password").await;
    freeze_user(&pool, "frozen@example.com").await;
    set_deletion_scheduled(
        &pool,
        "frozen@example.com",
        "2100-01-01T00:00:00.000Z",
        "user",
    )
    .await;
    let mock = start_google_mock(google_claims("sub-1", "frozen@example.com", true)).await;
    let app = test_app_with_google_login(pool.clone(), &mock).await;

    let response = run_login_flow(&app, LoginProvider::Google).await;

    assert_eq!(response.status(), StatusCode::SEE_OTHER);
    assert_eq!(
        location_header(&response),
        "/login?oauthError=account_frozen"
    );
    assert_eq!(
        count_identities(&pool, LoginProvider::Google, None).await,
        0
    );
    assert!(
        deletion_scheduled_at_of(&pool, "frozen@example.com")
            .await
            .is_some()
    );
}

/// 凍結中でも、正しいパスワードなら 429 ではなく凍結中であることが返る
/// (再試行のたびに上限を消費すると、理由を伝える前に 429 になってしまう)。
#[sqlx::test]
async fn frozen_login_attempts_do_not_consume_the_rate_limit(pool: SqlitePool) {
    insert_user(&pool, "kyoko", "password").await;
    freeze_user(&pool, "kyoko").await;
    let app = test_app(pool).await;

    for _ in 0..8 {
        let response = send(&app, login_request("kyoko", "password")).await;
        assert_error(response, StatusCode::FORBIDDEN, "account_frozen").await;
    }
}

/// 凍結されていても、正しいパスワードで来れば本人が起点の削除予約は取り消す (本人が取り消せる
/// 経路がここしか無いため、凍結が記録の消滅を確定させてしまわないように)。
#[sqlx::test]
async fn frozen_login_still_cancels_a_scheduled_deletion(pool: SqlitePool) {
    insert_user(&pool, "kyoko", "password").await;
    freeze_user(&pool, "kyoko").await;
    set_deletion_scheduled(&pool, "kyoko", "2100-01-01T00:00:00.000Z", "user").await;
    let app = test_app(pool.clone()).await;

    let response = send(&app, login_request("kyoko", "password")).await;

    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    assert_eq!(deletion_scheduled_at_of(&pool, "kyoko").await, None);
}

/// 紐付け済みの Google アカウントで来た場合も同じ。未連携の場合は
/// `google_callback_does_not_link_a_frozen_account` で確かめる。
#[sqlx::test]
async fn frozen_google_login_cancels_deletion_only_when_already_linked(pool: SqlitePool) {
    let mock = start_google_mock(google_claims("sub-1", "kyoko@example.com", true)).await;
    let app = test_app_with_google_login(pool.clone(), &mock).await;
    // 1回目のログインで紐付けを作ってから、凍結して削除を予約する。
    assert_eq!(
        location_header(&run_login_flow(&app, LoginProvider::Google).await),
        "/"
    );
    freeze_user(&pool, "kyoko@example.com").await;
    set_deletion_scheduled(
        &pool,
        "kyoko@example.com",
        "2100-01-01T00:00:00.000Z",
        "user",
    )
    .await;

    let response = run_login_flow(&app, LoginProvider::Google).await;

    assert_eq!(
        location_header(&response),
        "/login?oauthError=account_frozen"
    );
    assert_eq!(
        deletion_scheduled_at_of(&pool, "kyoko@example.com").await,
        None
    );
}
