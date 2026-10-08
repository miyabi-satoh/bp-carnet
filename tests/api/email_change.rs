//! メールアドレスの変更 (docs/authentication.md)。

use super::*;

fn request_email_change(
    cookie: &str,
    new_email: &str,
    current_password: Option<&str>,
) -> Request<Body> {
    post_json(
        "/api/v1/account/email",
        Some(cookie),
        json!({ "newEmail": new_email, "currentPassword": current_password }),
    )
}

/// 新しいアドレスにリンクを送り、リンクの先で確定するとユーザーIDが変わる。古いアドレスに知らせる。
#[sqlx::test]
async fn email_change_moves_the_account_to_the_new_address(pool: SqlitePool) {
    insert_user(&pool, "kyoko@example.com", "password").await;
    let (app, sent) = test_app_with_mail(pool).await;
    let cookie = login_and_get_cookie(&app, "kyoko@example.com", "password").await;

    let response = send(
        &app,
        request_email_change(&cookie, " New@Example.com ", Some("password")),
    )
    .await;
    assert_eq!(response.status(), StatusCode::NO_CONTENT);
    let mails = wait_for_mails(&sent, 1).await;
    assert_eq!(mails[0].to, "new@example.com");
    let token = link_token(&mails[0], "/change-email");

    let check = send(
        &app,
        post_json(
            "/api/v1/auth/email-change/check",
            None,
            json!({ "token": token }),
        ),
    )
    .await;
    assert_eq!(check.status(), StatusCode::OK);
    assert_eq!(json_body(check).await["newEmail"], "new@example.com");

    let complete = send(
        &app,
        post_json(
            "/api/v1/auth/email-change/complete",
            None,
            json!({ "token": token }),
        ),
    )
    .await;
    assert_eq!(complete.status(), StatusCode::NO_CONTENT);

    // 同じリンクは2度使えない。
    let again = send(
        &app,
        post_json(
            "/api/v1/auth/email-change/complete",
            None,
            json!({ "token": token }),
        ),
    )
    .await;
    assert_eq!(again.status(), StatusCode::BAD_REQUEST);

    let me = send(&app, get("/api/v1/auth/me", Some(&cookie))).await;
    assert_eq!(json_body(me).await["username"], "new@example.com");
    login_and_get_cookie(&app, "new@example.com", "password").await;

    let mails = wait_for_mails(&sent, 2).await;
    assert_eq!(mails[1].to, "kyoko@example.com");
    assert!(
        mails[1].body.contains("new@example.com"),
        "{}",
        mails[1].body
    );
}

/// 確認済みのアカウントが使うアドレスには、リンクではなく変えられないことを知らせる。応答は同じ。
#[sqlx::test]
async fn email_change_to_a_used_address_sends_a_notice_instead_of_a_link(pool: SqlitePool) {
    insert_user(&pool, "kyoko@example.com", "password").await;
    insert_user(&pool, "taken@example.com", "password").await;
    let (app, sent) = test_app_with_mail(pool).await;
    let cookie = login_and_get_cookie(&app, "kyoko@example.com", "password").await;

    let response = send(
        &app,
        request_email_change(&cookie, "taken@example.com", Some("password")),
    )
    .await;

    assert_eq!(response.status(), StatusCode::NO_CONTENT);
    let mails = wait_for_mails(&sent, 1).await;
    assert_eq!(mails[0].to, "taken@example.com");
    assert!(!mails[0].body.contains("token="), "{}", mails[0].body);
}

/// パスワードを持つアカウントは、今のパスワードが合わなければ送らない。今と同じアドレスも断る。
#[sqlx::test]
async fn email_change_requires_the_current_password_and_a_new_address(pool: SqlitePool) {
    insert_user(&pool, "kyoko@example.com", "password").await;
    let (app, sent) = test_app_with_mail(pool).await;
    let cookie = login_and_get_cookie(&app, "kyoko@example.com", "password").await;

    for (name, email, password, code) in [
        (
            "wrong password",
            "new@example.com",
            Some("wrong"),
            "incorrect_password",
        ),
        (
            "missing password",
            "new@example.com",
            None,
            "incorrect_password",
        ),
        (
            "same address in different case",
            "Kyoko@Example.com",
            Some("password"),
            "validation_error",
        ),
    ] {
        let response = send(&app, request_email_change(&cookie, email, password)).await;
        assert_error_case(response, StatusCode::UNPROCESSABLE_ENTITY, code, name).await;
    }
    assert!(sent_mails(&sent).is_empty());
}
