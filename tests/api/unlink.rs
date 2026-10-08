//! 連携の解除 (docs/authentication.md)。

use super::*;

async fn linked_providers_in_me(app: &Router, cookie: &str) -> Value {
    let response = send(app, get("/api/v1/auth/me", Some(cookie))).await;
    json_body(response).await["linkedProviders"].clone()
}

fn unlink_request(provider: &str, cookie: Option<&str>) -> Request<Body> {
    delete(&format!("/api/v1/account/identities/{provider}"), cookie)
}

/// パスワードがあれば、連携を外してもログインできる。外した後は `/auth/me` の一覧からも消える。
#[sqlx::test]
async fn unlink_removes_the_link_of_an_account_with_a_password(pool: SqlitePool) {
    insert_user(&pool, "kyoko@example.com", "password").await;
    let mock = start_google_mock(google_claims("sub-1", "kyoko@example.com", true)).await;
    let app = test_app_with_google_login(pool.clone(), &mock).await;
    let cookie = session_cookie(&run_login_flow(&app, LoginProvider::Google).await);
    assert_eq!(
        linked_providers_in_me(&app, &cookie).await,
        json!(["google"])
    );

    let response = send(&app, unlink_request("google", Some(&cookie))).await;

    assert_eq!(response.status(), StatusCode::NO_CONTENT);
    assert_eq!(
        count_identities(&pool, LoginProvider::Google, None).await,
        0
    );
    assert_eq!(linked_providers_in_me(&app, &cookie).await, json!([]));
    let response = send(&app, login_request("kyoko@example.com", "password")).await;
    assert_eq!(response.status(), StatusCode::OK);
}

/// パスワードも他の連携も無いアカウントは、最後のログイン方法を外せない。
#[sqlx::test]
async fn unlink_of_the_only_login_method_is_rejected(pool: SqlitePool) {
    let mock = start_google_mock(google_claims("sub-1", "kyoko@example.com", true)).await;
    let app = test_app_with_google_login(pool.clone(), &mock).await;
    let cookie = session_cookie(&run_login_flow(&app, LoginProvider::Google).await);

    let response = send(&app, unlink_request("google", Some(&cookie))).await;

    assert_error(
        response,
        StatusCode::CONFLICT,
        "cannot_unlink_last_login_method",
    )
    .await;
    assert_eq!(
        count_identities(&pool, LoginProvider::Google, None).await,
        1
    );
}

#[sqlx::test]
async fn unlink_of_a_provider_that_is_not_linked_returns_404(pool: SqlitePool) {
    insert_user(&pool, "kyoko@example.com", "password").await;
    let app = test_app(pool).await;
    let cookie = login_and_get_cookie(&app, "kyoko@example.com", "password").await;

    for provider in ["google", "line", "twitter"] {
        let response = send(&app, unlink_request(provider, Some(&cookie))).await;
        assert_eq!(response.status(), StatusCode::NOT_FOUND, "{provider}");
    }
}

/// LINE の連携を解除すると、LINE 側の連携も取り消す (退会時と同じ)。
#[sqlx::test]
async fn unlink_revokes_the_line_link(pool: SqlitePool) {
    insert_user(&pool, "kyoko@example.com", "password").await;
    let mock = start_line_mock(line_id_token("U1", Some("kyoko@example.com"))).await;
    let app = test_app_with_line_login(pool.clone(), &mock).await;
    let cookie = session_cookie(&run_login_flow(&app, LoginProvider::Line).await);

    let response = send(&app, unlink_request("line", Some(&cookie))).await;

    assert_eq!(response.status(), StatusCode::NO_CONTENT);
    assert_eq!(count_identities(&pool, LoginProvider::Line, None).await, 0);
    assert_eq!(
        *mock
            .deauthorize_requests
            .lock()
            .expect("mutex lock should not be poisoned"),
        [(
            "Bearer channel-access-token".to_string(),
            json!({ "userAccessToken": "refreshed-access-token" })
        )]
    );
}

/// 取り消せなくても、解除は成立する。
#[sqlx::test]
async fn unlink_succeeds_even_when_the_line_link_cannot_be_revoked(pool: SqlitePool) {
    insert_user(&pool, "kyoko@example.com", "password").await;
    let mock = start_line_mock(line_id_token("U1", Some("kyoko@example.com"))).await;
    let app = test_app_with_line_login(pool.clone(), &mock).await;
    let cookie = session_cookie(&run_login_flow(&app, LoginProvider::Line).await);
    *mock
        .deauthorize_status
        .lock()
        .expect("mutex lock should not be poisoned") = StatusCode::BAD_REQUEST;

    let response = send(&app, unlink_request("line", Some(&cookie))).await;

    assert_eq!(response.status(), StatusCode::NO_CONTENT);
    assert_eq!(count_identities(&pool, LoginProvider::Line, None).await, 0);
}

/// 外せなかったときは、LINE への取り消しも送らない。
#[sqlx::test]
async fn rejected_unlink_does_not_revoke_the_line_link(pool: SqlitePool) {
    let mock = start_line_mock(line_id_token("U1", Some("kyoko@example.com"))).await;
    let app = test_app_with_line_login(pool.clone(), &mock).await;
    let cookie = session_cookie(&run_login_flow(&app, LoginProvider::Line).await);

    let response = send(&app, unlink_request("line", Some(&cookie))).await;

    assert_eq!(response.status(), StatusCode::CONFLICT);
    assert!(
        mock.deauthorize_requests
            .lock()
            .expect("mutex lock should not be poisoned")
            .is_empty()
    );
}
