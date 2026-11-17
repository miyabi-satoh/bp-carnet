//! 管理画面に入るときの確認コード (docs/authentication.md「管理者の確認コード」)。照合の決まり (使い回し・鍵の違い)
//! は `src/admin_totp.rs` の単体テストで確かめ、ここでは入口を通して管理 API の守りにつながることを確かめる。

use bp_carnet::admin_totp::AdminTotp;
use totp_rs::{Builder, Secret};

use super::*;

const KEY: &str = "test-admin-totp-key";

async fn app_with_admin_code(pool: SqlitePool) -> (Router, Arc<Mutex<Vec<SentMail>>>) {
    let (mailer, sent) = Mailer::recording("http://localhost:3010");
    let mut config = test_config();
    config.alerts.to = "ops@example.com".to_string();
    let services = Services {
        mailer: Arc::new(mailer),
        admin_totp: Some(AdminTotp::with_key(KEY)),
        ..disabled_services()
    };
    (build_test_app(pool, &config, services).await, sent)
}

/// 管理者を端末で登録したのと同じ状態にし、認証アプリの秘密 (base32) を返す。
async fn enroll(pool: &SqlitePool, username: &str) -> String {
    let user_id = user_id_of(pool, username).await;
    let enrollment = AdminTotp::new_enrollment(username);
    AdminTotp::with_key(KEY)
        .save(pool, user_id, &enrollment, 0)
        .await
        .expect("登録できる");
    enrollment.secret_base32()
}

/// 認証アプリが今出すコード。
fn current_code(secret: &str, username: &str) -> String {
    Builder::new()
        .with_secret(Secret::try_from_base32(secret).expect("base32"))
        .with_issuer(Some("BP Carnet"))
        .with_account_name(username)
        .build()
        .expect("RFC の既定")
        .generate_current()
        .to_string()
}

fn wrong_code(secret: &str, username: &str) -> &'static str {
    if current_code(secret, username) == "000000" {
        "111111"
    } else {
        "000000"
    }
}

fn verify_request(cookie: &str, code: &str) -> Request<Body> {
    post_json(
        "/api/v1/admin/verification",
        Some(cookie),
        json!({ "code": code }),
    )
}

/// 確認コードを通すまでは管理 API を使えず、通すと使える。間違いは運営者に知らせる。
#[sqlx::test]
async fn admin_apis_require_the_code_on_the_web(pool: SqlitePool) {
    insert_admin(&pool, "admin@example.com", "password").await;
    let (app, sent) = app_with_admin_code(pool.clone()).await;
    let cookie = login_and_drain_notice(&app, &sent, "admin@example.com", "password").await;

    let before = send(&app, get("/api/v1/admin/users", Some(&cookie))).await;
    assert_error_case(
        before,
        StatusCode::FORBIDDEN,
        "admin_verification_required",
        "通す前",
    )
    .await;
    let status = send(&app, get("/api/v1/admin/verification", Some(&cookie))).await;
    assert_eq!(
        json_body(status).await,
        json!({ "required": true, "enrolled": false, "verified": false })
    );
    let not_enrolled = send(&app, verify_request(&cookie, "123456")).await;
    assert_error_case(
        not_enrolled,
        StatusCode::CONFLICT,
        "admin_code_not_enrolled",
        "登録前",
    )
    .await;

    let secret = enroll(&pool, "admin@example.com").await;
    let wrong = send(
        &app,
        verify_request(&cookie, wrong_code(&secret, "admin@example.com")),
    )
    .await;
    assert_error_case(wrong, StatusCode::BAD_REQUEST, "invalid_admin_code", "誤り").await;
    let mails = wait_for_mails(&sent, 1).await;
    assert_eq!(mails[0].to, "ops@example.com");
    assert!(
        mails[0]
            .subject
            .contains("確認コードが間違って入力されました"),
        "{}",
        mails[0].subject
    );

    let passed = send(
        &app,
        verify_request(&cookie, &current_code(&secret, "admin@example.com")),
    )
    .await;
    assert_eq!(passed.status(), StatusCode::NO_CONTENT);
    let after = send(&app, get("/api/v1/admin/users", Some(&cookie))).await;
    assert_eq!(after.status(), StatusCode::OK);
    let status = send(&app, get("/api/v1/admin/verification", Some(&cookie))).await;
    assert_eq!(json_body(status).await["verified"], true);
}

/// 通した印はログインごと。別のブラウザーのログインでは、もう一度求める。アプリのトークンでも同じ。
#[sqlx::test]
async fn the_code_is_required_per_login_including_the_app(pool: SqlitePool) {
    insert_admin(&pool, "admin@example.com", "password").await;
    let (app, sent) = app_with_admin_code(pool.clone()).await;
    let secret = enroll(&pool, "admin@example.com").await;
    let first = login_and_drain_notice(&app, &sent, "admin@example.com", "password").await;
    let passed = send(
        &app,
        verify_request(&first, &current_code(&secret, "admin@example.com")),
    )
    .await;
    assert_eq!(passed.status(), StatusCode::NO_CONTENT);

    let second = login_and_get_cookie(&app, "admin@example.com", "password").await;
    let other_login = send(&app, get("/api/v1/admin/users", Some(&second))).await;
    assert_eq!(other_login.status(), StatusCode::FORBIDDEN);

    let login = send(
        &app,
        app_request(
            Method::POST,
            "/api/v1/app/auth/login",
            None,
            Some(json!({"username": "admin@example.com", "password": "password"})),
        ),
    )
    .await;
    let token = json_body(login).await["token"]
        .as_str()
        .expect("token")
        .to_string();
    let before = send(
        &app,
        app_request(Method::GET, "/api/v1/admin/users", Some(&token), None),
    )
    .await;
    assert_eq!(before.status(), StatusCode::FORBIDDEN);
    // ブラウザーで通したのと同じ30秒の区切りのコードは使い回しとして断られる。次の区切りを待たずに済むよう、
    // 使った印 (last_step) を戻してから通す。
    sqlx::query!("UPDATE admin_totp SET last_step = NULL")
        .execute(&pool)
        .await
        .expect("戻せる");
    let verified = send(
        &app,
        app_request(
            Method::POST,
            "/api/v1/admin/verification",
            Some(&token),
            Some(json!({ "code": current_code(&secret, "admin@example.com") })),
        ),
    )
    .await;
    assert_eq!(verified.status(), StatusCode::NO_CONTENT);
    let after = send(
        &app,
        app_request(Method::GET, "/api/v1/admin/users", Some(&token), None),
    )
    .await;
    assert_eq!(after.status(), StatusCode::OK);
}

/// 管理者でなければ、確認コードの API も使えない。
#[sqlx::test]
async fn the_verification_apis_reject_non_admins(pool: SqlitePool) {
    insert_user(&pool, "member", "password").await;
    let (app, _) = app_with_admin_code(pool).await;
    let cookie = login_and_get_cookie(&app, "member", "password").await;

    for response in [
        send(&app, get("/api/v1/admin/verification", Some(&cookie))).await,
        send(&app, verify_request(&cookie, "123456")).await,
    ] {
        assert_error_case(response, StatusCode::FORBIDDEN, "forbidden", "一般ユーザー").await;
    }
}

/// 確認コードを求めない構成では、通したものとして返す (画面は入力を出さない)。
#[sqlx::test]
async fn without_the_key_the_code_is_not_required(pool: SqlitePool) {
    insert_admin(&pool, "admin", "password").await;
    let app = test_app(pool).await;
    let cookie = login_and_get_cookie(&app, "admin", "password").await;

    let status = send(&app, get("/api/v1/admin/verification", Some(&cookie))).await;
    assert_eq!(
        json_body(status).await,
        json!({ "required": false, "enrolled": false, "verified": true })
    );
}
