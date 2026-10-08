//! アプリ内課金 (docs/payments.md)。

use super::*;

/// (環境 `production`・`sandbox`, 取引 ID) → (ステータス, 取引)。
type AppStoreTransactions = Arc<Mutex<HashMap<(String, String), (u16, Value)>>>;

/// App Store Server API の Get Transaction Info を模すローカルサーバー。
/// `/{environment}/inApps/v1/transactions/{id}` で、登録した取引を返す (無ければ 404)。
struct AppStoreMock {
    base_url: String,
    transactions: AppStoreTransactions,
    calls: Arc<std::sync::atomic::AtomicUsize>,
}

impl AppStoreMock {
    fn put(&self, environment: &str, transaction: Value) {
        self.put_status(environment, transaction, 200);
    }

    fn put_status(&self, environment: &str, transaction: Value, status: u16) {
        let id = transaction["transactionId"]
            .as_str()
            .expect("取引 ID がいる")
            .to_string();
        self.transactions
            .lock()
            .expect("mutex lock should not be poisoned")
            .insert((environment.to_string(), id), (status, transaction));
    }

    fn calls(&self) -> usize {
        self.calls.load(std::sync::atomic::Ordering::SeqCst)
    }
}

async fn start_app_store_mock() -> AppStoreMock {
    let transactions: AppStoreTransactions = Arc::default();
    let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let app = Router::new()
        .route(
            "/{environment}/inApps/v1/transactions/{id}",
            get_route(
                |State((transactions, calls)): State<(
                    AppStoreTransactions,
                    Arc<std::sync::atomic::AtomicUsize>,
                )>,
                 headers: axum::http::HeaderMap,
                 Path((environment, id)): Path<(String, String)>| async move {
                    calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                    assert!(
                        headers
                            .get(header::AUTHORIZATION)
                            .and_then(|value| value.to_str().ok())
                            .is_some_and(|value| value.starts_with("Bearer ")),
                        "JWT を Bearer で送るはず"
                    );
                    let found = transactions
                        .lock()
                        .expect("mutex lock should not be poisoned")
                        .get(&(environment, id))
                        .cloned();
                    match found {
                        Some((status, transaction)) => (
                            StatusCode::from_u16(status).expect("status"),
                            Json(json!({ "signedTransactionInfo": unsigned_jwt(&transaction) })),
                        )
                            .into_response(),
                        None => (
                            StatusCode::NOT_FOUND,
                            Json(json!({ "errorCode": 4040010, "errorMessage": "Transaction id not found." })),
                        )
                            .into_response(),
                    }
                },
            ),
        )
        .with_state((transactions.clone(), calls.clone()));
    AppStoreMock {
        base_url: serve_mock(app).await,
        transactions,
        calls,
    }
}

/// 買い足しの商品として正しい取引。
fn app_store_transaction(id: &str, environment: &str, app_account_token: Option<&str>) -> Value {
    json!({
        "transactionId": id,
        "originalTransactionId": id,
        "bundleId": APPLE_BUNDLE_ID,
        "productId": app_store::PRODUCT_ID,
        "type": "Consumable",
        "quantity": 1,
        "purchaseDate": 1_790_000_000_000_i64,
        "signedDate": 1_790_000_000_000_i64,
        "appAccountToken": app_account_token,
        "environment": environment,
        "storefront": "JPN",
    })
}

/// `mock` の URL でアプリ内課金を有効にしたアプリ。通知の送り元は 17.0.0.0/8 だけを受け付ける。
async fn test_app_with_app_store(pool: SqlitePool, mock: &AppStoreMock) -> Router {
    let app_store = AppStoreClient::for_test(
        app_store::Settings {
            issuer_id: "issuer".to_string(),
            key_id: "KEY123".to_string(),
            private_key_pem: APPLE_TEST_PRIVATE_KEY.to_string(),
            bundle_id: APPLE_BUNDLE_ID.to_string(),
        },
        app_store::Endpoints {
            production: format!("{}/production", mock.base_url),
            sandbox: format!("{}/sandbox", mock.base_url),
        },
        vec!["17.0.0.0/8".parse().expect("CIDR")],
    );
    let services = Services {
        app_store,
        ..disabled_services()
    };
    build_test_app(pool, &test_config(), services).await
}

async fn apple_account_token_of(app: &Router, cookie: &str) -> String {
    let response = send(
        app,
        post_json(
            "/api/v1/payments/apple/account-token",
            Some(cookie),
            json!({}),
        ),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    json_body(response).await["appAccountToken"]
        .as_str()
        .expect("文字列のはず")
        .to_string()
}

async fn submit_apple_transaction(
    app: &Router,
    cookie: &str,
    transaction_id: &str,
    environment: &str,
) -> Response {
    send(
        app,
        post_json(
            "/api/v1/payments/apple/transactions",
            Some(cookie),
            json!({ "transactionId": transaction_id, "environment": environment }),
        ),
    )
    .await
}

fn apple_notification(notification_type: &str, environment: &str, transaction_id: &str) -> Value {
    json!({
        "signedPayload": unsigned_jwt(&json!({
            "notificationType": notification_type,
            "notificationUUID": "uuid",
            "version": "2.0",
            "data": {
                "bundleId": APPLE_BUNDLE_ID,
                "environment": environment,
                "signedTransactionInfo": unsigned_jwt(&json!({ "transactionId": transaction_id })),
            },
        })),
    })
}

async fn send_apple_notification(app: &Router, peer: &str, body: Value) -> Response {
    send(
        app,
        from_peer(
            post_json("/api/v1/payments/apple/notifications", None, body),
            peer,
        ),
    )
    .await
}

/// アプリ内課金の取引の行。
#[derive(Debug, PartialEq)]
struct AppleGrant {
    /// アカウントを消した後も購入の記録は残るので、持ち主が無い行もある。
    user_id: Option<i64>,
    remaining_milli_yen: i64,
    /// 返金で消した残り。
    revoked_milli_yen: Option<i64>,
    price_yen: i64,
    apple_environment: String,
}

async fn apple_grant(pool: &SqlitePool, transaction_id: &str) -> Option<AppleGrant> {
    sqlx::query_as!(
        AppleGrant,
        r#"SELECT user_id, remaining_milli_yen, revoked_milli_yen, price_yen, apple_environment AS "apple_environment!: String"
           FROM ocr_quota_grants WHERE apple_transaction_id = ?"#,
        transaction_id
    )
    .fetch_optional(pool)
    .await
    .expect("grants should be readable")
}

const TOPUP: i64 = TEST_TOPUP_GRANT_MILLI_YEN;
const TOPUP_PRICE_YEN: i64 = bp_carnet::payments::ocr_quota::TOPUP_PRICE_YEN;

/// 設定が無ければアプリ内課金の口は 503 で、`appStoreTopupAvailable` も `false`。
#[sqlx::test]
async fn apple_purchases_are_disabled_without_the_app_store_key(pool: SqlitePool) {
    insert_user(&pool, "kyoko", "password").await;
    set_ocr_budget(&pool, 80, 0).await;
    let app = test_app(pool).await;
    let cookie = login_and_get_cookie(&app, "kyoko", "password").await;

    let response = send(
        &app,
        post_json(
            "/api/v1/payments/apple/account-token",
            Some(&cookie),
            json!({}),
        ),
    )
    .await;
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    let response = submit_apple_transaction(&app, &cookie, "1000", "Production").await;
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    let response = send(&app, get("/api/v1/ocr/status", Some(&cookie))).await;
    assert_eq!(json_body(response).await["appStoreTopupAvailable"], false);
}

/// appAccountToken はアカウントごとに固定の UUID (小文字)。無料の枠に上限があれば買い足せると返す。
#[sqlx::test]
async fn apple_account_token_is_fixed_per_account(pool: SqlitePool) {
    insert_user(&pool, "kyoko", "password").await;
    insert_user(&pool, "ren", "password").await;
    set_ocr_budget(&pool, 80, 0).await;
    let mock = start_app_store_mock().await;
    let app = test_app_with_app_store(pool, &mock).await;
    let kyoko = login_and_get_cookie(&app, "kyoko", "password").await;
    let ren = login_and_get_cookie(&app, "ren", "password").await;

    let token = apple_account_token_of(&app, &kyoko).await;
    assert_eq!(token.len(), 36);
    assert_eq!(token, token.to_ascii_lowercase());
    assert_eq!(token.as_bytes()[14], b'4', "version 4 のはず");
    assert_eq!(apple_account_token_of(&app, &kyoko).await, token);
    assert_ne!(apple_account_token_of(&app, &ren).await, token);

    let response = send(&app, get("/api/v1/ocr/status", Some(&kyoko))).await;
    let body = json_body(response).await;
    assert_eq!(body["appStoreTopupAvailable"], true);
    assert_eq!(body["topupAvailable"], false, "Stripe は無効のまま");
}

/// 取引は Apple から取り直し、appAccountToken の持ち主に1回だけ足す。ほかの人が同じ取引を送っても、
/// 枠は持ち主のまま。
#[sqlx::test]
async fn apple_transaction_grants_the_owner_once(pool: SqlitePool) {
    insert_user(&pool, "kyoko", "password").await;
    insert_user(&pool, "ren", "password").await;
    let kyoko_id = user_id_of(&pool, "kyoko").await;
    let mock = start_app_store_mock().await;
    let app = test_app_with_app_store(pool.clone(), &mock).await;
    let kyoko = login_and_get_cookie(&app, "kyoko", "password").await;
    let ren = login_and_get_cookie(&app, "ren", "password").await;
    let token = apple_account_token_of(&app, &kyoko).await;
    // StoreKit は大文字で返すことがあるが、持ち主は同じ。
    mock.put(
        "production",
        app_store_transaction(
            "2000000001",
            "Production",
            Some(&token.to_ascii_uppercase()),
        ),
    );

    let response = submit_apple_transaction(&app, &kyoko, "2000000001", "Production").await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(json_body(response).await["status"], "granted");
    let response = submit_apple_transaction(&app, &kyoko, "2000000001", "Production").await;
    assert_eq!(json_body(response).await["status"], "already_granted");
    let response = submit_apple_transaction(&app, &ren, "2000000001", "Production").await;
    assert_eq!(json_body(response).await["status"], "already_granted");

    assert_eq!(
        apple_grant(&pool, "2000000001").await,
        Some(AppleGrant {
            user_id: Some(kyoko_id),
            remaining_milli_yen: TOPUP,
            revoked_milli_yen: None,
            price_yen: TOPUP_PRICE_YEN,
            apple_environment: "Production".to_string(),
        })
    );
    assert_eq!(paid_remaining(&pool, kyoko_id).await, TOPUP);
    // storefront (JPN) から、国内の買い手として残す。
    assert_eq!(
        grant_domestic(&pool, "apple_transaction_id", "2000000001").await,
        Some(1)
    );
}

/// アプリが添えた環境で見つからなければ、もう一方の環境で引く。行には見つかった環境を残す。
#[sqlx::test]
async fn apple_transaction_falls_back_to_the_other_environment(pool: SqlitePool) {
    insert_user(&pool, "kyoko", "password").await;
    let mock = start_app_store_mock().await;
    let app = test_app_with_app_store(pool.clone(), &mock).await;
    let cookie = login_and_get_cookie(&app, "kyoko", "password").await;
    let token = apple_account_token_of(&app, &cookie).await;
    mock.put(
        "sandbox",
        app_store_transaction("2000000002", "Sandbox", Some(&token)),
    );

    let response = submit_apple_transaction(&app, &cookie, "2000000002", "Production").await;
    assert_eq!(json_body(response).await["status"], "granted");
    assert_eq!(
        apple_grant(&pool, "2000000002")
            .await
            .map(|grant| grant.apple_environment),
        Some("Sandbox".to_string())
    );
}

/// 両方の環境で見つからない・Apple の API が失敗したときは、足さないと決めずに 503 を返す (アプリは取引を終えない)。
#[sqlx::test]
async fn apple_transaction_is_left_undecided_when_apple_cannot_confirm_it(pool: SqlitePool) {
    insert_user(&pool, "kyoko", "password").await;
    let mock = start_app_store_mock().await;
    let app = test_app_with_app_store(pool.clone(), &mock).await;
    let cookie = login_and_get_cookie(&app, "kyoko", "password").await;
    let token = apple_account_token_of(&app, &cookie).await;

    let response = submit_apple_transaction(&app, &cookie, "2000000003", "Production").await;
    assert_error(
        response,
        StatusCode::SERVICE_UNAVAILABLE,
        "app_store_unavailable",
    )
    .await;

    mock.put_status(
        "production",
        app_store_transaction("2000000003", "Production", Some(&token)),
        500,
    );
    let response = submit_apple_transaction(&app, &cookie, "2000000003", "Production").await;
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(apple_grant(&pool, "2000000003").await, None);
}

/// 買い足しの商品と合わない取引・持ち主のいない取引・数字でない取引 ID は、ずっと足さない (アプリは取引を終える)。
/// 商品と合うかの場合ごとの確かめは `app_store::is_topup_product` の単体テスト。
#[sqlx::test]
async fn apple_transaction_rejects_other_products_and_unknown_owners(pool: SqlitePool) {
    insert_user(&pool, "kyoko", "password").await;
    let mock = start_app_store_mock().await;
    let app = test_app_with_app_store(pool.clone(), &mock).await;
    let cookie = login_and_get_cookie(&app, "kyoko", "password").await;
    let token = apple_account_token_of(&app, &cookie).await;
    let mut other_product = app_store_transaction("2000000004", "Production", Some(&token));
    other_product["productId"] = json!("com.example.other");
    mock.put("production", other_product);
    mock.put(
        "production",
        app_store_transaction("2000000006", "Production", None),
    );
    mock.put(
        "production",
        app_store_transaction(
            "2000000007",
            "Production",
            Some("00000000-0000-4000-8000-000000000000"),
        ),
    );

    for id in ["2000000004", "2000000006", "2000000007"] {
        let response = submit_apple_transaction(&app, &cookie, id, "Production").await;
        assert_eq!(response.status(), StatusCode::OK, "{id}");
        assert_eq!(json_body(response).await["status"], "rejected", "{id}");
        assert_eq!(apple_grant(&pool, id).await, None, "{id}");
    }

    let calls = mock.calls();
    let response = submit_apple_transaction(&app, &cookie, "../1", "Production").await;
    assert_eq!(json_body(response).await["status"], "rejected");
    assert_eq!(mock.calls(), calls, "数字でない取引 ID は Apple に送らない");
}

/// 読めない・問い合わせと合わない取引と、App Store の外の環境 (`Xcode`) の取引も、ずっと足さない。
/// 503 にするとアプリが取引を終えず、起動のたびに送り直し続けるため。
#[sqlx::test]
async fn apple_transaction_rejects_unusable_transactions(pool: SqlitePool) {
    insert_user(&pool, "kyoko", "password").await;
    let mock = start_app_store_mock().await;
    let app = test_app_with_app_store(pool.clone(), &mock).await;
    let cookie = login_and_get_cookie(&app, "kyoko", "password").await;
    let token = apple_account_token_of(&app, &cookie).await;
    mock.put(
        "production",
        app_store_transaction("2000000011", "Xcode", Some(&token)),
    );
    mock.put(
        "production",
        app_store_transaction("2000000012", "Sandbox", Some(&token)),
    );
    let mut bad_date = app_store_transaction("2000000013", "Production", Some(&token));
    bad_date["purchaseDate"] = json!(i64::MAX);
    mock.put("production", bad_date);

    for id in ["2000000011", "2000000012", "2000000013"] {
        let response = submit_apple_transaction(&app, &cookie, id, "Production").await;
        assert_eq!(response.status(), StatusCode::OK, "{id}");
        assert_eq!(json_body(response).await["status"], "rejected", "{id}");
        assert_eq!(apple_grant(&pool, id).await, None, "{id}");
    }

    let calls = mock.calls();
    let response = submit_apple_transaction(&app, &cookie, "2000000014", "Xcode").await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(json_body(response).await["status"], "rejected");
    assert_eq!(mock.calls(), calls, "Xcode の取引は Apple に問い合わせない");
}

/// 取引の確かめは、ユーザーごとに1分10回まで。ほかのユーザーは妨げない。
#[sqlx::test]
async fn apple_transaction_is_rate_limited_per_user(pool: SqlitePool) {
    insert_user(&pool, "kyoko", "password").await;
    insert_user(&pool, "ren", "password").await;
    let mock = start_app_store_mock().await;
    let app = test_app_with_app_store(pool, &mock).await;
    let kyoko = login_and_get_cookie(&app, "kyoko", "password").await;
    let ren = login_and_get_cookie(&app, "ren", "password").await;

    for _ in 0..10 {
        let response = submit_apple_transaction(&app, &kyoko, "1", "Xcode").await;
        assert_eq!(response.status(), StatusCode::OK);
    }
    let response = submit_apple_transaction(&app, &kyoko, "1", "Xcode").await;
    assert_error(response, StatusCode::TOO_MANY_REQUESTS, "too_many_requests").await;
    let response = submit_apple_transaction(&app, &ren, "1", "Xcode").await;
    assert_eq!(response.status(), StatusCode::OK);
}

/// 返金の通知で残りを消し、返金の取り消しの通知で戻す。同じ通知が重なっても結果は変わらない。
/// 行に反映したものより古い取引 (`signedDate`) が後から届いても、行は変えない。
#[sqlx::test]
async fn apple_notifications_revoke_and_restore_the_quota(pool: SqlitePool) {
    insert_user(&pool, "kyoko", "password").await;
    let kyoko_id = user_id_of(&pool, "kyoko").await;
    let mock = start_app_store_mock().await;
    let app = test_app_with_app_store(pool.clone(), &mock).await;
    let cookie = login_and_get_cookie(&app, "kyoko", "password").await;
    let token = apple_account_token_of(&app, &cookie).await;
    let purchase = app_store_transaction("2000000008", "Production", Some(&token));
    mock.put("production", purchase.clone());
    submit_apple_transaction(&app, &cookie, "2000000008", "Production").await;
    // 一部を使った後で返金される。
    sqlx::query!(
        "UPDATE ocr_quota_grants SET remaining_milli_yen = 30000 WHERE apple_transaction_id = '2000000008'"
    )
    .execute(&pool)
    .await
    .expect("テストデータを更新できるはず");

    let mut refunded = purchase.clone();
    refunded["revocationDate"] = json!(1_790_100_000_000_i64);
    refunded["signedDate"] = json!(1_790_100_000_000_i64);
    mock.put("production", refunded.clone());
    for _ in 0..2 {
        let response = send_apple_notification(
            &app,
            "17.1.2.3:443",
            apple_notification("REFUND", "Production", "2000000008"),
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            apple_grant(&pool, "2000000008").await,
            Some(AppleGrant {
                user_id: Some(kyoko_id),
                remaining_milli_yen: 0,
                revoked_milli_yen: Some(30000),
                price_yen: TOPUP_PRICE_YEN,
                apple_environment: "Production".to_string(),
            })
        );
    }

    mock.put("production", purchase.clone());
    // 返金を反映するより前に取った古い「有効」が、アプリの送信やほかの通知で後から届いても戻さない。
    let response = submit_apple_transaction(&app, &cookie, "2000000008", "Production").await;
    assert_eq!(json_body(response).await["status"], "rejected");
    let response = send_apple_notification(
        &app,
        "17.1.2.3:443",
        apple_notification("ONE_TIME_CHARGE", "Production", "2000000008"),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        apple_grant(&pool, "2000000008").await,
        Some(AppleGrant {
            user_id: Some(kyoko_id),
            remaining_milli_yen: 0,
            revoked_milli_yen: Some(30000),
            price_yen: TOPUP_PRICE_YEN,
            apple_environment: "Production".to_string(),
        })
    );

    let mut reversed = purchase;
    reversed["signedDate"] = json!(1_790_200_000_000_i64);
    mock.put("production", reversed);
    for _ in 0..2 {
        let response = send_apple_notification(
            &app,
            "17.1.2.3:443",
            apple_notification("REFUND_REVERSED", "Production", "2000000008"),
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            apple_grant(&pool, "2000000008").await,
            Some(AppleGrant {
                user_id: Some(kyoko_id),
                remaining_milli_yen: 30000,
                revoked_milli_yen: None,
                price_yen: TOPUP_PRICE_YEN,
                apple_environment: "Production".to_string(),
            })
        );
    }

    // 戻した後に、それより前に取った古い「取り消し済み」が届いても、また消さない。
    mock.put("production", refunded);
    let response = submit_apple_transaction(&app, &cookie, "2000000008", "Production").await;
    assert_eq!(json_body(response).await["status"], "already_granted");
    let response = send_apple_notification(
        &app,
        "17.1.2.3:443",
        apple_notification("REFUND", "Production", "2000000008"),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        apple_grant(&pool, "2000000008").await,
        Some(AppleGrant {
            user_id: Some(kyoko_id),
            remaining_milli_yen: 30000,
            revoked_milli_yen: None,
            price_yen: TOPUP_PRICE_YEN,
            apple_environment: "Production".to_string(),
        })
    );
}

/// アカウントを消した後に返金や返金の取り消しが届いたら、残した購入の記録の印を合わせる。
#[sqlx::test]
async fn apple_refunds_after_the_account_is_deleted_update_the_kept_purchase(pool: SqlitePool) {
    insert_user(&pool, "kyoko", "password").await;
    let kyoko_id = user_id_of(&pool, "kyoko").await;
    let mock = start_app_store_mock().await;
    let app = test_app_with_app_store(pool.clone(), &mock).await;
    let cookie = login_and_get_cookie(&app, "kyoko", "password").await;
    let token = apple_account_token_of(&app, &cookie).await;
    let purchase = app_store_transaction("2000000020", "Production", Some(&token));
    mock.put("production", purchase.clone());
    submit_apple_transaction(&app, &cookie, "2000000020", "Production").await;
    sqlx::query!("DELETE FROM users WHERE id = ?", kyoko_id)
        .execute(&pool)
        .await
        .expect("アカウントを消せるはず");

    let mut refunded = purchase.clone();
    refunded["revocationDate"] = json!(1_790_100_000_000_i64);
    refunded["signedDate"] = json!(1_790_100_000_000_i64);
    mock.put("production", refunded);
    let response = send_apple_notification(
        &app,
        "17.1.2.3:443",
        apple_notification("REFUND", "Production", "2000000020"),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        apple_grant(&pool, "2000000020").await,
        Some(AppleGrant {
            user_id: None,
            remaining_milli_yen: 0,
            revoked_milli_yen: Some(0),
            price_yen: TOPUP_PRICE_YEN,
            apple_environment: "Production".to_string(),
        })
    );

    // 返金が取り消されたら印を外す。残りは戻さない。
    let mut reversed = purchase;
    reversed["signedDate"] = json!(1_790_200_000_000_i64);
    mock.put("production", reversed);
    let response = send_apple_notification(
        &app,
        "17.1.2.3:443",
        apple_notification("REFUND_REVERSED", "Production", "2000000020"),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        apple_grant(&pool, "2000000020").await,
        Some(AppleGrant {
            user_id: None,
            remaining_milli_yen: 0,
            revoked_milli_yen: None,
            price_yen: TOPUP_PRICE_YEN,
            apple_environment: "Production".to_string(),
        })
    );
}

/// 返金が付与より先に届いても、返金済みの取引に枠を残さない。アプリから届かなかった購入は通知で足す。
#[sqlx::test]
async fn apple_notifications_handle_purchases_the_app_did_not_send(pool: SqlitePool) {
    insert_user(&pool, "kyoko", "password").await;
    let kyoko_id = user_id_of(&pool, "kyoko").await;
    let mock = start_app_store_mock().await;
    let app = test_app_with_app_store(pool.clone(), &mock).await;
    let cookie = login_and_get_cookie(&app, "kyoko", "password").await;
    let token = apple_account_token_of(&app, &cookie).await;

    mock.put(
        "sandbox",
        app_store_transaction("2000000009", "Sandbox", Some(&token)),
    );
    let response = send_apple_notification(
        &app,
        "17.1.2.3:443",
        apple_notification("ONE_TIME_CHARGE", "Sandbox", "2000000009"),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        apple_grant(&pool, "2000000009").await,
        Some(AppleGrant {
            user_id: Some(kyoko_id),
            remaining_milli_yen: TOPUP,
            revoked_milli_yen: None,
            price_yen: TOPUP_PRICE_YEN,
            apple_environment: "Sandbox".to_string(),
        })
    );

    let mut refunded = app_store_transaction("2000000010", "Production", Some(&token));
    refunded["revocationDate"] = json!(1_790_100_000_000_i64);
    mock.put("production", refunded);
    send_apple_notification(
        &app,
        "17.1.2.3:443",
        apple_notification("REFUND", "Production", "2000000010"),
    )
    .await;
    let response = submit_apple_transaction(&app, &cookie, "2000000010", "Production").await;
    assert_eq!(json_body(response).await["status"], "rejected");
    assert_eq!(
        apple_grant(&pool, "2000000010").await,
        Some(AppleGrant {
            user_id: Some(kyoko_id),
            remaining_milli_yen: 0,
            revoked_milli_yen: Some(TOPUP),
            price_yen: TOPUP_PRICE_YEN,
            apple_environment: "Production".to_string(),
        })
    );
}

/// Apple の範囲の外からの通知は取り直さずに 403。形が不正なら 400、関係の無い通知は何もせず 200、
/// 取引が見つからなければ再送させる 503。
#[sqlx::test]
async fn apple_notifications_are_checked_before_calling_apple(pool: SqlitePool) {
    let mock = start_app_store_mock().await;
    let app = test_app_with_app_store(pool, &mock).await;

    let response = send_apple_notification(
        &app,
        "18.1.2.3:443",
        apple_notification("REFUND", "Production", "2000000011"),
    )
    .await;
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    assert_eq!(mock.calls(), 0);

    let response =
        send_apple_notification(&app, "17.1.2.3:443", json!({ "signedPayload": "x" })).await;
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);

    for body in [
        apple_notification("CONSUMPTION_REQUEST", "Production", "2000000011"),
        apple_notification("REFUND", "Xcode", "2000000011"),
    ] {
        let response = send_apple_notification(&app, "17.1.2.3:443", body).await;
        assert_eq!(response.status(), StatusCode::OK);
    }
    assert_eq!(mock.calls(), 0);

    let response = send_apple_notification(
        &app,
        "17.1.2.3:443",
        apple_notification("REFUND", "Production", "2000000011"),
    )
    .await;
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
}
