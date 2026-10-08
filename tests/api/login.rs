//! ID とパスワードのログイン・セッション・レート制限。

use super::*;

/// https で公開する設定 (`secure_cookie`) では、セッション Cookie を `__Host-` の条件どおりに発行し、
/// その Cookie でログイン中として扱う。同じ登録ドメインの別のサブドメインから差し替えられないようにするため。
#[sqlx::test]
async fn secure_login_issues_host_prefixed_session_cookie(pool: SqlitePool) {
    insert_user(&pool, "admin", "password").await;
    let mut config = test_config();
    config.session.secure_cookie = true;
    let app = test_app_with_config(pool, &config).await;

    let response = send(&app, login_request("admin", "password")).await;
    assert_eq!(response.status(), StatusCode::OK);

    let cookies = set_cookie_values(&response);
    let set_cookie =
        find_set_cookie(&cookies, "__Host-id").expect("__Host-id cookie should be set");
    assert!(set_cookie.contains("Secure"));
    assert!(set_cookie.split("; ").any(|attr| attr == "Path=/"));
    assert!(!set_cookie.contains("Domain"));
    assert!(find_set_cookie(&cookies, "id").is_none());

    let cookie = cookie_pair(&response, "__Host-id");
    let me_response = send(&app, get("/api/v1/auth/me", Some(&cookie))).await;
    assert_eq!(me_response.status(), StatusCode::OK);

    // ログアウトで消す Set-Cookie にも Secure が要る。無いとブラウザが捨て、Cookie が残る。
    let logout_response = send(
        &app,
        request(
            Method::POST,
            "/api/v1/auth/logout",
            Some(&cookie),
            &[],
            None,
        ),
    )
    .await;
    assert_eq!(logout_response.status(), StatusCode::NO_CONTENT);
    let cookies = set_cookie_values(&logout_response);
    let removal =
        find_set_cookie(&cookies, "__Host-id").expect("__Host-id removal cookie should be set");
    assert!(removal.split("; ").any(|attr| attr == "Secure"));
    assert!(removal.split("; ").any(|attr| attr == "Path=/"));
    assert!(removal.contains("Max-Age=0"));
}

/// 無いユーザーも、パスワード違いと同じ 401 `invalid_credentials` にする (ユーザーの有無を教えないため)。
#[sqlx::test]
async fn login_failure_returns_401_invalid_credentials(pool: SqlitePool) {
    insert_user(&pool, "admin", "password").await;
    let app = test_app(pool).await;

    for (name, username, password) in [
        ("wrong password", "admin", "wrong-password"),
        ("unknown user", "nobody", "whatever"),
    ] {
        let response = send(&app, login_request(username, password)).await;
        assert_error_case(
            response,
            StatusCode::UNAUTHORIZED,
            "invalid_credentials",
            name,
        )
        .await;
    }
}

/// 本文を読めないときも、axum の素の rejection ではなく共通の envelope (`AppJson` 経由) で返す。
#[sqlx::test]
async fn login_with_an_unreadable_body_returns_envelope(pool: SqlitePool) {
    let app = test_app(pool).await;
    let raw = |content_type: &str, body: String| {
        Request::post("/api/v1/auth/login")
            .header(header::CONTENT_TYPE, content_type)
            .body(Body::from(body))
            .expect("failed to build request")
    };

    for (name, request, expected) in [
        (
            "malformed JSON",
            raw("application/json", "{not json".to_owned()),
            StatusCode::BAD_REQUEST,
        ),
        (
            "missing field",
            post_json("/api/v1/auth/login", None, json!({"username": "admin"})),
            StatusCode::UNPROCESSABLE_ENTITY,
        ),
        (
            "wrong content type",
            raw(
                "text/plain",
                json!({"username": "admin", "password": "password"}).to_string(),
            ),
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
        ),
    ] {
        let response = send(&app, request).await;
        assert_error_case(response, expected, "invalid_request_body", name).await;
    }
}

#[sqlx::test]
async fn tampered_cookie_is_rejected(pool: SqlitePool) {
    let app = test_app(pool).await;

    // 署名の無い/偽造した Cookie 値は有効なセッションに結び付かず、未ログイン扱いになる。
    let response = send(&app, get("/api/v1/auth/me", Some("id=forged-session-id"))).await;

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

/// ログイン → `/me` で本人確認 → ログアウト → `/me` が再び 401 になる、
/// という一連のセッションライフサイクルを検証する。
#[sqlx::test]
async fn login_me_logout_flow(pool: SqlitePool) {
    insert_user(&pool, "admin", "password").await;
    let app = test_app(pool).await;

    let login_response = send(&app, login_request("admin", "password")).await;
    assert_eq!(login_response.status(), StatusCode::OK);
    let cookie = session_cookie(&login_response);
    let body = json_body(login_response).await;
    assert_eq!(body["username"], "admin");
    assert!(body["id"].is_i64());

    let me_response = send(&app, get("/api/v1/auth/me", Some(&cookie))).await;
    assert_eq!(me_response.status(), StatusCode::OK);
    let me = json_body(me_response).await;
    assert_eq!(me["username"], "admin");
    assert_eq!(me["timezone"], "Asia/Tokyo");

    let logout_response = send(
        &app,
        request(
            Method::POST,
            "/api/v1/auth/logout",
            Some(&cookie),
            &[],
            None,
        ),
    )
    .await;
    assert_eq!(logout_response.status(), StatusCode::NO_CONTENT);

    let me_after_logout = send(&app, get("/api/v1/auth/me", Some(&cookie))).await;
    assert_eq!(me_after_logout.status(), StatusCode::UNAUTHORIZED);
}

/// `/auth/me` が権限を返し、管理者と一般ユーザーを区別できることを確認する。
#[sqlx::test]
async fn me_returns_role_of_logged_in_user(pool: SqlitePool) {
    insert_user(&pool, "member", "password").await;
    insert_admin(&pool, "boss", "password").await;
    let app = test_app(pool).await;

    for (username, expected_role) in [("member", "user"), ("boss", "admin")] {
        let cookie = login_and_get_cookie(&app, username, "password").await;
        let response = send(&app, get("/api/v1/auth/me", Some(&cookie))).await;
        assert_eq!(response.status(), StatusCode::OK);
        let me = json_body(response).await;
        assert_eq!(me["role"], expected_role);
    }
}

/// 接続元が分からない構成 (`ConnectInfo` も信頼済みプロキシの `X-Forwarded-For` も無い) では、
/// ユーザー名ごとに数え、連続したログイン失敗が一定回数を超えると 429 になる (docs/authentication.md)。
/// この構成では、同じ名前を試し続ける第三者と本人を区別できない (仕様)。
#[sqlx::test]
async fn login_is_rate_limited_after_repeated_failures(pool: SqlitePool) {
    insert_user(&pool, "admin", "password").await;
    let app = test_app(pool).await;

    // レートリミットの上限 (5回/分) に達するまでは 401。
    for _ in 0..5 {
        let response = send(&app, login_request("admin", "wrong-password")).await;
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    // 上限を超えると、正しいパスワードでも 429 になる。
    let limited_response = send(&app, login_request("admin", "password")).await;
    assert_error(
        limited_response,
        StatusCode::TOO_MANY_REQUESTS,
        "too_many_requests",
    )
    .await;
}

/// 接続元ごとに数えるため、第三者が本人の名前で試し続けても、別の接続元の本人はログインできる
/// (docs/authentication.md)。IPv6 は /64 ごとに数えるため、同じ /64 の中で送信元を変えながら
/// 試しても止まる。/64 にまとめる場合ごとの確かめは `src/forwarded.rs` の `rate_limit_key` のテスト。
#[sqlx::test]
async fn login_rate_limit_counts_per_client(pool: SqlitePool) {
    insert_user(&pool, "admin", "password").await;
    let app = test_app(pool).await;

    for i in 1..=5 {
        let response = send(
            &app,
            from_peer(
                login_request("admin", "wrong-password"),
                &format!("[2001:db8:1:2::{i}]:50000"),
            ),
        )
        .await;
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    // 試し続けた接続元は、同じ /64 の別のアドレスから正しいパスワードを送っても止まる。
    let attacker = send(
        &app,
        from_peer(
            login_request("admin", "password"),
            "[2001:db8:1:2::ffff]:50000",
        ),
    )
    .await;
    assert_eq!(attacker.status(), StatusCode::TOO_MANY_REQUESTS);

    // 別の接続元の本人は巻き込まれない。
    let owner = send(
        &app,
        from_peer(
            login_request("admin", "password"),
            "[2001:db8:1:3::1]:50000",
        ),
    )
    .await;
    assert_eq!(owner.status(), StatusCode::OK);
}

/// 名前を変えながら試しても、同じ接続元からは1分に30回で止まる。他の接続元は巻き込まない。
#[sqlx::test]
async fn login_rate_limit_stops_a_client_trying_many_usernames(pool: SqlitePool) {
    insert_user(&pool, "admin", "password").await;
    let app = test_app(pool).await;

    for i in 0..30 {
        let response = send(
            &app,
            from_peer(
                login_request(&format!("nobody{i}"), "wrong-password"),
                "192.0.2.10:50000",
            ),
        )
        .await;
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    let limited = send(
        &app,
        from_peer(login_request("admin", "password"), "192.0.2.10:50000"),
    )
    .await;
    assert_eq!(limited.status(), StatusCode::TOO_MANY_REQUESTS);

    let other = send(
        &app,
        from_peer(login_request("admin", "password"), "198.51.100.20:50000"),
    )
    .await;
    assert_eq!(other.status(), StatusCode::OK);
}

/// 認証成功時はレート制限の消費が解放されるため、ユーザー単位の上限 (既定5回/分) を
/// 超える回数、正しいパスワードでログインし続けても 429 にならないことを確認する。
#[sqlx::test]
async fn successful_login_does_not_consume_rate_limit(pool: SqlitePool) {
    insert_user(&pool, "admin", "password").await;
    let app = test_app(pool).await;

    for _ in 0..10 {
        let response = send(&app, login_request("admin", "password")).await;
        assert_eq!(response.status(), StatusCode::OK);
    }
}

/// ログイン後に DB からユーザーが削除された場合、有効なセッション Cookie を持っていても
/// `/me` が 401 を返すことを確認する (`AuthUser` がセッションだけでなく DB 上の実在も確認する)。
#[sqlx::test]
async fn me_returns_401_after_user_is_deleted(pool: SqlitePool) {
    insert_user(&pool, "admin", "password").await;
    let app = test_app(pool.clone()).await;

    let login_response = send(&app, login_request("admin", "password")).await;
    let cookie = session_cookie(&login_response);

    sqlx::query!("DELETE FROM users WHERE username = ?", "admin")
        .execute(&pool)
        .await
        .expect("failed to delete test user");

    let me_response = send(&app, get("/api/v1/auth/me", Some(&cookie))).await;
    assert_eq!(me_response.status(), StatusCode::UNAUTHORIZED);
}
