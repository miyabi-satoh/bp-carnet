//! メールのリンクで進める手続き (サインアップ・パスワードの再設定) と規約への同意。

use super::*;

fn signup_request(email: &str) -> Request<Body> {
    post_json("/api/v1/auth/signup", None, json!({ "email": email }))
}

fn complete_signup_request(token: &str, password: &str) -> Request<Body> {
    post_json(
        "/api/v1/auth/signup/complete",
        None,
        json!({ "token": token, "password": password }),
    )
}

/// サインアップ → 確認リンクでパスワードを設定 → そのままログイン済みになり、以降は
/// そのパスワードでもログインできる。
#[sqlx::test]
async fn signup_and_completing_the_link_logs_in(pool: SqlitePool) {
    let (app, sent) = test_app_with_mail(pool).await;

    let response = send(&app, signup_request(" Kyoko@Example.com ")).await;
    assert_eq!(response.status(), StatusCode::NO_CONTENT);

    let mails = wait_for_mails(&sent, 1).await;
    assert_eq!(mails.len(), 1);
    assert_eq!(mails[0].to, "kyoko@example.com");
    assert!(
        mails[0]
            .body
            .contains("http://localhost:3010/verify-email?token="),
        "{}",
        mails[0].body
    );
    assert!(mails[0].body.contains(LINK_HELP), "{}", mails[0].body);

    // 確認前はログインできない (パスワードをまだ持たない)。
    let before = send(&app, login_request("kyoko@example.com", "password")).await;
    assert_eq!(before.status(), StatusCode::UNAUTHORIZED);

    let response = send(
        &app,
        complete_signup_request(&link_token(&mails[0], "/verify-email"), "password"),
    )
    .await;
    assert_eq!(response.status(), StatusCode::NO_CONTENT);
    let cookie = session_cookie(&response);

    let me = send(&app, get("/api/v1/auth/me", Some(&cookie))).await;
    assert_eq!(json_body(me).await["username"], "kyoko@example.com");

    login_and_get_cookie(&app, "kyoko@example.com", "password").await;
}

/// 確認済みのメールアドレスでも応答は同じで、メールにはリンクではなくログインの案内を載せる。
#[sqlx::test]
async fn signup_for_a_registered_email_sends_a_notice_instead_of_a_link(pool: SqlitePool) {
    insert_user(&pool, "kyoko@example.com", "password").await;
    let (app, sent) = test_app_with_mail(pool).await;

    let response = send(&app, signup_request("kyoko@example.com")).await;

    assert_eq!(response.status(), StatusCode::NO_CONTENT);
    let mails = wait_for_mails(&sent, 1).await;
    assert_eq!(mails.len(), 1);
    assert!(!mails[0].body.contains("token="), "{}", mails[0].body);
    assert!(
        mails[0].body.contains("http://localhost:3010/login"),
        "{}",
        mails[0].body
    );
    // 既存のパスワードは変わらない。
    login_and_get_cookie(&app, "kyoko@example.com", "password").await;
}

/// メールアドレスとして読めないもの、読めてもユーザーIDとして使えない文字 (空白) を含むものは弾き、
/// メールも送らずアカウントも作らない。
#[sqlx::test]
async fn signup_rejects_unusable_emails(pool: SqlitePool) {
    let (app, sent) = test_app_with_mail(pool.clone()).await;

    for email in ["not an address", "\"a b\"@example.com"] {
        let response = send(&app, signup_request(email)).await;
        assert_error_case(
            response,
            StatusCode::UNPROCESSABLE_ENTITY,
            "validation_error",
            email,
        )
        .await;
    }
    assert!(sent_mails(&sent).is_empty());
    assert_eq!(count_users(&pool).await, 0);
}

/// 同じ接続元からの要求は10分に5回まで (`AttemptRateLimiter::for_mail_requests`)。
#[sqlx::test]
async fn signup_is_rate_limited_per_client(pool: SqlitePool) {
    let (app, sent) = test_app_with_mail(pool).await;

    for i in 0..5 {
        let response = send(&app, signup_request(&format!("user{i}@example.com"))).await;
        assert_eq!(response.status(), StatusCode::NO_CONTENT);
    }
    let response = send(&app, signup_request("user5@example.com")).await;

    assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(wait_for_mails(&sent, 5).await.len(), 5);
}

#[sqlx::test]
async fn complete_signup_rejects_an_unknown_token(pool: SqlitePool) {
    let (app, _sent) = test_app_with_mail(pool).await;

    let response = send(&app, complete_signup_request("unknown-token", "password")).await;

    assert_error(response, StatusCode::BAD_REQUEST, "invalid_email_token").await;
}

fn check_link_request(path: &str, token: &str) -> Request<Body> {
    post_json(path, None, json!({ "token": token }))
}

/// リンクがまだ使えるかを尋ねた応答のステータスとエラーコード。
async fn check_link(app: &Router, path: &str, token: &str) -> (StatusCode, serde_json::Value) {
    let response = send(app, check_link_request(path, token)).await;
    let status = response.status();
    let code = if status == StatusCode::NO_CONTENT {
        serde_json::Value::Null
    } else {
        json_body(response).await["error"]["code"].clone()
    };
    (status, code)
}

/// 確認リンクは、尋ねるだけでは使用済みにならず、使った後は使えないと答える。
/// 用途違い・期限切れのリンクの判定は `email_token::is_usable` の単体テストで確かめる。
#[sqlx::test]
async fn check_signup_link_reports_usability_without_consuming(pool: SqlitePool) {
    let (app, sent) = test_app_with_mail(pool).await;
    send(&app, signup_request("kyoko@example.com")).await;
    let token = link_token(&wait_for_mails(&sent, 1).await[0], "/verify-email");
    let path = "/api/v1/auth/signup/check";

    assert_eq!(
        check_link(&app, path, &token).await.0,
        StatusCode::NO_CONTENT
    );
    let response = send(&app, complete_signup_request(&token, "password")).await;
    assert_eq!(response.status(), StatusCode::NO_CONTENT);

    assert_eq!(
        check_link(&app, path, &token).await,
        (StatusCode::BAD_REQUEST, json!("invalid_email_token"))
    );
}

/// 未確認のアカウントとして受け付け、確認リンクのトークンを返す。
async fn register_token(pool: &SqlitePool, username: &str) -> String {
    match bp_carnet::signup::register(pool, username, "unusable", "1")
        .await
        .expect("受け付けられるはず")
    {
        bp_carnet::signup::Registration::Unverified { token } => token,
        other => panic!("未確認として発行されるはず: {other:?}"),
    }
}

/// 他人に先にサインアップされた (確認されていない) メールアドレスでも、本人は Google で
/// ログインでき、同じ行を引き継ぐ。先に送られていた確認リンクは使えなくなる。引き継いだ行には
/// 規約への同意を残す。
#[sqlx::test]
async fn google_login_takes_over_an_unverified_signup(pool: SqlitePool) {
    let token = register_token(&pool, "shared@example.com").await;
    let mock = start_google_mock(google_claims("sub-1", "shared@example.com", true)).await;
    let app = test_app_with_google_login(pool.clone(), &mock).await;

    let response = run_login_flow(&app, LoginProvider::Google).await;

    assert_eq!(response.status(), StatusCode::SEE_OTHER);
    assert_eq!(location_header(&response), "/");
    assert_eq!(count_users(&pool).await, 1);
    let completed = send(&app, complete_signup_request(&token, "password")).await;
    assert_eq!(completed.status(), StatusCode::BAD_REQUEST);
    assert_eq!(
        terms_agreement(&pool, "shared@example.com")
            .await
            .0
            .as_deref(),
        Some(bp_carnet::terms::VERSION)
    );
}

/// 確認待ちのアカウントは、パスワードのハッシュが一致しても ID/PW ログインを通さない。
#[sqlx::test]
async fn login_rejects_an_unverified_account(pool: SqlitePool) {
    bp_carnet::signup::register(&pool, "kyoko@example.com", "unusable", "1")
        .await
        .expect("受け付けられるはず");
    let hash = auth::hash_password("password").expect("ハッシュ化できるはず");
    sqlx::query!(
        "UPDATE users SET password_hash = ?, password_usable = 1 WHERE username = 'kyoko@example.com'",
        hash
    )
    .execute(&pool)
    .await
    .expect("テストデータを更新できるはず");
    let app = test_app(pool).await;

    let response = send(&app, login_request("kyoko@example.com", "password")).await;

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[sqlx::test]
async fn admin_users_exclude_unverified_signups(pool: SqlitePool) {
    insert_admin(&pool, "admin", "password").await;
    bp_carnet::signup::register(&pool, "pending@example.com", "unusable", "1")
        .await
        .expect("受け付けられるはず");
    let app = test_app(pool).await;
    let cookie = login_and_get_cookie(&app, "admin", "password").await;

    let list = send(&app, get("/api/v1/admin/users", Some(&cookie))).await;

    let usernames: Vec<Value> = json_body(list).await["users"]
        .as_array()
        .expect("users は配列のはず")
        .iter()
        .map(|user| user["username"].clone())
        .collect();
    assert_eq!(usernames, vec![json!("admin")]);
}

/// 管理者は、確認待ちのアカウント (一覧に出ない) と同じメールアドレスのユーザーも作れる。
#[sqlx::test]
async fn admin_create_user_replaces_an_unverified_signup(pool: SqlitePool) {
    insert_admin(&pool, "admin", "password").await;
    bp_carnet::signup::register(&pool, "pending@example.com", "unusable", "1")
        .await
        .expect("受け付けられるはず");
    let app = test_app(pool.clone()).await;
    let cookie = login_and_get_cookie(&app, "admin", "password").await;

    let response = send(
        &app,
        create_user_request(
            &cookie,
            json!({ "username": "pending@example.com", "password": "password" }),
        ),
    )
    .await;

    assert_eq!(response.status(), StatusCode::CREATED);
    assert_eq!(count_users(&pool).await, 2);
    login_and_get_cookie(&app, "pending@example.com", "password").await;
}

/// 同じリンクで並行に送られても、パスワードを設定できるのは1件だけ (2度目は使えない)。
#[sqlx::test]
async fn complete_signup_accepts_only_one_of_concurrent_requests(pool: SqlitePool) {
    let (app, sent) = test_app_with_mail(pool).await;
    send(&app, signup_request("kyoko@example.com")).await;
    let token = link_token(&wait_for_mails(&sent, 1).await[0], "/verify-email");

    let (first, second) = tokio::join!(
        app.clone()
            .oneshot(complete_signup_request(&token, "password")),
        app.clone()
            .oneshot(complete_signup_request(&token, "another1")),
    );

    let mut statuses = [
        first.expect("oneshot request should not fail").status(),
        second.expect("oneshot request should not fail").status(),
    ];
    statuses.sort();
    assert_eq!(statuses, [StatusCode::NO_CONTENT, StatusCode::BAD_REQUEST]);
}

/// 未確認のアカウントは、一覧に出ないのと同じく、管理者のどの操作でも存在しないもの (404) として扱い、
/// 監査ログにも残さない。
#[sqlx::test]
async fn admin_operations_are_not_found_for_unverified_signups(pool: SqlitePool) {
    insert_admin(&pool, "admin", "password").await;
    bp_carnet::signup::register(&pool, "pending@example.com", "unusable", "1")
        .await
        .expect("受け付けられるはず");
    let pending_id = user_id_of(&pool, "pending@example.com").await;
    let app = test_app(pool.clone()).await;
    let cookie = login_and_get_cookie(&app, "admin", "password").await;

    let requests = [
        get(&format!("/api/v1/admin/users/{pending_id}"), Some(&cookie)),
        get(
            &format!("/api/v1/admin/users/{pending_id}/records"),
            Some(&cookie),
        ),
        set_frozen_request(&cookie, pending_id, true),
        put_json(
            &format!("/api/v1/admin/users/{pending_id}/ocr-limit"),
            Some(&cookie),
            json!({ "ocrBudgetYen": 5 }),
        ),
        admin_delete_request(&cookie, pending_id),
        admin_cancel_deletion_request(&cookie, pending_id),
        reset_password_request(&cookie, pending_id, "password"),
    ];
    for request in requests {
        let uri = format!("{} {}", request.method(), request.uri());
        let response = send(&app, request).await;
        assert_eq!(response.status(), StatusCode::NOT_FOUND, "{uri}");
    }
    assert!(audit_log_entries(&pool).await.is_empty());
}

/// 確認完了で Argon2 まで進む要求は、同じ接続元から1分に5回まで (`AttemptRateLimiter::with_defaults`)。
/// 上限で弾いた要求はリンクを使い済みにしない。
#[sqlx::test]
async fn complete_signup_is_rate_limited_per_client(pool: SqlitePool) {
    // サインアップ受付のレートリミットにかからないよう、トークンは直接発行する。
    let mut tokens = Vec::new();
    for i in 0..6 {
        tokens.push(register_token(&pool, &format!("user{i}@example.com")).await);
    }
    let (app, _sent) = test_app_with_mail(pool.clone()).await;

    for token in &tokens[..5] {
        let response = send(&app, complete_signup_request(token, "password")).await;
        assert_eq!(response.status(), StatusCode::NO_CONTENT);
    }
    let response = send(&app, complete_signup_request(&tokens[5], "password")).await;

    assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
    let still_usable = bp_carnet::email_token::consume(
        &pool,
        &tokens[5],
        bp_carnet::email_token::Purpose::VerifyEmail,
    )
    .await
    .expect("照合できるはず");
    assert!(
        still_usable.is_some(),
        "弾いた要求でリンクを潰してはいけない"
    );
}

/// 使えないトークンの要求は枠を消費しない。でたらめなトークンで全体の枠を使い切り、他の人の
/// 確認完了を止められないようにするため。
#[sqlx::test]
async fn complete_signup_does_not_count_unusable_tokens(pool: SqlitePool) {
    let (app, sent) = test_app_with_mail(pool).await;

    for _ in 0..40 {
        let response = send(&app, complete_signup_request("unknown-token", "password")).await;
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }
    send(&app, signup_request("kyoko@example.com")).await;
    let response = send(
        &app,
        complete_signup_request(
            &link_token(&wait_for_mails(&sent, 1).await[0], "/verify-email"),
            "password",
        ),
    )
    .await;

    assert_eq!(response.status(), StatusCode::NO_CONTENT);
}

/// 再設定のリンクは、尋ねるだけでは使用済みにならず、使った後は使えないと答える。
#[sqlx::test]
async fn check_password_reset_link_reports_usability_without_consuming(pool: SqlitePool) {
    insert_user(&pool, "kyoko@example.com", "password").await;
    let (app, sent) = test_app_with_mail(pool).await;
    send(&app, password_reset_request("kyoko@example.com")).await;
    let token = link_token(&wait_for_mails(&sent, 1).await[0], "/reset-password");
    let path = "/api/v1/auth/password-reset/check";

    assert_eq!(
        check_link(&app, path, &token).await.0,
        StatusCode::NO_CONTENT
    );
    let response = send(
        &app,
        complete_password_reset_request(&token, "new-password"),
    )
    .await;
    assert_eq!(response.status(), StatusCode::NO_CONTENT);

    assert_eq!(
        check_link(&app, path, &token).await,
        (StatusCode::BAD_REQUEST, json!("invalid_email_token"))
    );
}

/// リンクの先で新しいパスワードを設定すると、古いパスワードとセッションは使えなくなる。
/// 再設定ではログインさせない。
#[sqlx::test]
async fn password_reset_link_sets_a_new_password_and_cuts_sessions(pool: SqlitePool) {
    insert_user(&pool, "kyoko@example.com", "password").await;
    let (app, sent) = test_app_with_mail(pool).await;
    let old_cookie = login_and_get_cookie(&app, "kyoko@example.com", "password").await;

    let response = send(&app, password_reset_request("Kyoko@Example.com")).await;
    assert_eq!(response.status(), StatusCode::NO_CONTENT);
    let mails = wait_for_mails(&sent, 1).await;
    assert_eq!(mails[0].to, "kyoko@example.com");
    assert!(mails[0].body.contains(LINK_HELP), "{}", mails[0].body);

    let response = send(
        &app,
        complete_password_reset_request(&link_token(&mails[0], "/reset-password"), "new-password"),
    )
    .await;
    assert_eq!(response.status(), StatusCode::NO_CONTENT);

    let me = send(&app, get("/api/v1/auth/me", Some(&old_cookie))).await;
    assert_eq!(me.status(), StatusCode::UNAUTHORIZED);
    let old_login = send(&app, login_request("kyoko@example.com", "password")).await;
    assert_eq!(old_login.status(), StatusCode::UNAUTHORIZED);
    login_and_get_cookie(&app, "kyoko@example.com", "new-password").await;
}

/// アカウントが無いアドレスでも応答は同じで、メールは送らない。
#[sqlx::test]
async fn password_reset_sends_nothing_without_an_account(pool: SqlitePool) {
    let (app, sent) = test_app_with_mail(pool).await;

    let response = send(&app, password_reset_request("nobody@example.com")).await;

    assert_eq!(response.status(), StatusCode::NO_CONTENT);
    assert_mail_count_settles(&sent, 0).await;
}

/// 確認前のアカウントには、再設定のリンクの代わりに確認リンクを送り直す。直後の申し込み直しでは送らない。
#[sqlx::test]
async fn password_reset_resends_the_verification_link_to_an_unverified_account(pool: SqlitePool) {
    bp_carnet::signup::register(&pool, "pending@example.com", "unusable", "1")
        .await
        .expect("受け付けられるはず");
    // 確認メールを失くして、送り直せる間隔が過ぎた状態にする。
    sqlx::query!("DELETE FROM email_tokens")
        .execute(&pool)
        .await
        .expect("消せるはず");
    let (app, sent) = test_app_with_mail(pool).await;

    for _ in 0..2 {
        let response = send(&app, password_reset_request("pending@example.com")).await;
        assert_eq!(response.status(), StatusCode::NO_CONTENT);
    }
    let mails = wait_for_mails(&sent, 1).await;
    assert_mail_count_settles(&sent, 1).await;
    assert_eq!(mails[0].to, "pending@example.com");
    assert!(
        mails[0].body.contains("まだ確認が済んでいません"),
        "{}",
        mails[0].body
    );

    let response = send(
        &app,
        complete_signup_request(&link_token(&mails[0], "/verify-email"), "new-password"),
    )
    .await;
    assert_eq!(response.status(), StatusCode::NO_CONTENT);
    login_and_get_cookie(&app, "pending@example.com", "new-password").await;
}

/// 使われていない再設定のリンクを直前に送っていれば、申し込み直されても発行し直さず、
/// 本人に届いたリンクはそのまま使える。
#[sqlx::test]
async fn password_reset_does_not_reissue_a_recent_link(pool: SqlitePool) {
    insert_user(&pool, "kyoko@example.com", "password").await;
    let (app, sent) = test_app_with_mail(pool).await;

    send(&app, password_reset_request("kyoko@example.com")).await;
    let mails = wait_for_mails(&sent, 1).await;
    let response = send(&app, password_reset_request("kyoko@example.com")).await;
    assert_eq!(response.status(), StatusCode::NO_CONTENT);
    assert_mail_count_settles(&sent, 1).await;

    let response = send(
        &app,
        complete_password_reset_request(&link_token(&mails[0], "/reset-password"), "new-password"),
    )
    .await;
    assert_eq!(response.status(), StatusCode::NO_CONTENT);
}

/// 使われていない確認リンクを直前に送っていれば、第三者に申し込み直されても発行し直さず
/// メールも送らない。本人に届いたリンクはそのまま使える。
#[sqlx::test]
async fn signup_does_not_reissue_a_recent_link(pool: SqlitePool) {
    let (app, sent) = test_app_with_mail(pool).await;

    send(&app, signup_request("kyoko@example.com")).await;
    let mails = wait_for_mails(&sent, 1).await;
    let response = send(&app, signup_request("kyoko@example.com")).await;
    assert_eq!(response.status(), StatusCode::NO_CONTENT);
    assert_mail_count_settles(&sent, 1).await;

    let response = send(
        &app,
        complete_signup_request(&link_token(&mails[0], "/verify-email"), "password"),
    )
    .await;
    assert_eq!(response.status(), StatusCode::NO_CONTENT);
}

/// 「オープンβテスト中」の表示は `[server] beta_notice` で決まり、既定は出さない。
#[sqlx::test]
async fn providers_report_the_beta_notice_only_when_enabled(pool: SqlitePool) {
    let mut beta = test_config();
    beta.server.beta_notice = true;
    for (config, expected) in [(test_config(), false), (beta, true)] {
        let app = test_app_with_config(pool.clone(), &config).await;
        assert_eq!(auth_providers(app).await["betaNotice"], expected);
    }
}

/// 問い合わせ先 (`[server] contact_url`) と規約の版を返す。
#[sqlx::test]
async fn providers_report_the_contact_url_and_the_terms_version(pool: SqlitePool) {
    let body = auth_providers(test_app(pool).await).await;
    assert_eq!(body["contactUrl"], "https://example.com/contact/");
    assert_eq!(body["termsVersion"], bp_carnet::terms::VERSION);
    assert_eq!(body["introUrl"], "");
}

/// サインアップでは、申し込んだ時点で同意した版と日時を残す。
#[sqlx::test]
async fn signup_records_the_terms_agreement(pool: SqlitePool) {
    let (app, sent) = test_app_with_mail_and_config(pool.clone(), &test_config()).await;

    let response = send(&app, signup_request("kyoko@example.com")).await;
    assert_eq!(response.status(), StatusCode::NO_CONTENT);
    wait_for_mails(&sent, 1).await;

    let (version, agreed_at) = terms_agreement(&pool, "kyoko@example.com").await;
    assert_eq!(version.as_deref(), Some(bp_carnet::terms::VERSION));
    assert!(agreed_at.is_some());
}

/// Google で初めてログインすると、作ったアカウントに同意を残す。
#[sqlx::test]
async fn google_first_login_records_the_terms_agreement(pool: SqlitePool) {
    let mock = start_google_mock(google_claims("sub-1", "newuser@example.com", true)).await;
    let app = test_app_with_google_login_and_config(pool.clone(), &mock, &test_config()).await;

    let response = run_login_flow(&app, LoginProvider::Google).await;
    assert_eq!(location_header(&response), "/");

    let (version, agreed_at) = terms_agreement(&pool, "newuser@example.com").await;
    assert_eq!(version.as_deref(), Some(bp_carnet::terms::VERSION));
    assert!(agreed_at.is_some());
}

/// 確認済みの既存アカウントが Google でログインしても、同意の記録は変えない (登録ではないため)。
#[sqlx::test]
async fn google_login_of_an_existing_account_keeps_the_terms_agreement(pool: SqlitePool) {
    insert_user(&pool, "sakura@example.com", "password").await;
    let mock = start_google_mock(google_claims("sub-1", "sakura@example.com", true)).await;
    let app = test_app_with_google_login_and_config(pool.clone(), &mock, &test_config()).await;

    run_login_flow(&app, LoginProvider::Google).await;

    assert_eq!(
        terms_agreement(&pool, "sakura@example.com").await,
        (None, None)
    );
}
