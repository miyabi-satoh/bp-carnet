//! 決済 (Stripe、OCR 枠の買い足し。docs/payments.md)。Stripe の API は呼ばず、ローカルの `StripeMock` (`start_stripe_mock`) に差し替える。

use super::*;

#[derive(Clone)]
struct StripeMockState {
    create_requests: Arc<Mutex<Vec<HashMap<String, String>>>>,
    retrieve_count: Arc<std::sync::atomic::AtomicUsize>,
    sessions: Arc<Mutex<HashMap<String, Value>>>,
    next_id: Arc<std::sync::atomic::AtomicUsize>,
}

async fn stripe_create_session(
    State(state): State<StripeMockState>,
    Form(body): Form<HashMap<String, String>>,
) -> Json<Value> {
    let id = format!(
        "cs_test_{}",
        state
            .next_id
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst)
    );
    let user_id = body.get("metadata[user_id]").cloned().unwrap_or_default();
    let product = body.get("metadata[product]").cloned().unwrap_or_default();
    let session = json!({
        "id": id,
        "url": format!("https://checkout.stripe.test/{id}"),
        "status": "open",
        "payment_status": "unpaid",
        "mode": body.get("mode").cloned().unwrap_or_default(),
        "client_reference_id": body.get("client_reference_id").cloned(),
        "payment_intent": Value::Null,
        "amount_total": 300,
        "currency": "jpy",
        "line_items": {
            "data": [{
                "price": { "id": body.get("line_items[0][price]").cloned().unwrap_or_default() },
                "quantity": 1,
            }],
        },
        "metadata": { "user_id": user_id, "product": product },
    });
    state
        .sessions
        .lock()
        .expect("mutex lock should not be poisoned")
        .insert(id.clone(), session.clone());
    state
        .create_requests
        .lock()
        .expect("mutex lock should not be poisoned")
        .push(body);
    Json(session)
}

async fn stripe_retrieve_session(
    State(state): State<StripeMockState>,
    Path(id): Path<String>,
) -> Response {
    state
        .retrieve_count
        .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    match state
        .sessions
        .lock()
        .expect("mutex lock should not be poisoned")
        .get(&id)
    {
        Some(session) => Json(session.clone()).into_response(),
        None => StatusCode::NOT_FOUND.into_response(),
    }
}

async fn stripe_list_sessions(
    State(state): State<StripeMockState>,
    Query(params): Query<HashMap<String, String>>,
) -> Json<Value> {
    let payment_intent = params.get("payment_intent").cloned();
    let sessions = state
        .sessions
        .lock()
        .expect("mutex lock should not be poisoned");
    let data: Vec<Value> = sessions
        .values()
        .filter(|session| {
            payment_intent
                .as_deref()
                .is_none_or(|pi| session["payment_intent"].as_str() == Some(pi))
        })
        .cloned()
        .collect();
    Json(json!({ "data": data }))
}

/// Stripe の Checkout Session API (作成・取得・`payment_intent` での一覧) を模すローカルサーバー。
struct StripeMock {
    base_url: String,
    state: StripeMockState,
}

impl StripeMock {
    /// あらかじめ Session を登録する。Webhook・購入結果確認のテストで、Checkout 作成 API を
    /// 経由せず「Stripe 側に既にある Session」を装うために使う。
    fn seed_session(&self, session: Value) {
        let id = session["id"]
            .as_str()
            .expect("seeded session needs an id")
            .to_string();
        self.state
            .sessions
            .lock()
            .expect("mutex lock should not be poisoned")
            .insert(id, session);
    }

    fn last_create_request(&self) -> HashMap<String, String> {
        self.state
            .create_requests
            .lock()
            .expect("mutex lock should not be poisoned")
            .last()
            .cloned()
            .expect("create endpoint should have been called")
    }

    fn retrieve_request_count(&self) -> usize {
        self.state
            .retrieve_count
            .load(std::sync::atomic::Ordering::SeqCst)
    }
}

async fn start_stripe_mock() -> StripeMock {
    let state = StripeMockState {
        create_requests: Arc::new(Mutex::new(Vec::new())),
        retrieve_count: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
        sessions: Arc::new(Mutex::new(HashMap::new())),
        next_id: Arc::new(std::sync::atomic::AtomicUsize::new(1)),
    };
    let app = Router::new()
        .route(
            "/v1/checkout/sessions",
            post_route(stripe_create_session).get(stripe_list_sessions),
        )
        .route(
            "/v1/checkout/sessions/{id}",
            get_route(stripe_retrieve_session),
        )
        .with_state(state.clone());
    StripeMock {
        base_url: serve_mock(app).await,
        state,
    }
}

const STRIPE_WEBHOOK_SECRET_FOR_TEST: &str = "whsec_test_secret";

/// `mock` の URL で Stripe 決済を有効化した状態のアプリ。
async fn test_app_with_stripe(pool: SqlitePool, mock: &StripeMock) -> Router {
    test_app_with_stripe_client(pool, stripe_client_for(mock)).await
}

/// `mock` の URL で有効化した Stripe のクライアント (税率は付けない)。
fn stripe_client_for(mock: &StripeMock) -> StripeClient {
    StripeClient::for_test(
        "sk_test_x",
        STRIPE_WEBHOOK_SECRET_FOR_TEST,
        "price_test",
        "https://example.test/success?session_id={CHECKOUT_SESSION_ID}",
        "https://example.test/cancel",
        bp_carnet::payments::stripe::Endpoints {
            api_base: format!("{}/v1", mock.base_url),
        },
    )
}

async fn test_app_with_stripe_client(pool: SqlitePool, stripe: StripeClient) -> Router {
    let services = Services {
        stripe,
        ..disabled_services()
    };
    build_test_app(pool, &test_config(), services).await
}

/// Webhook を送るリクエスト。署名は送信するのと同じ生バイト列に対して計算する必要があるため、
/// JSON の文字列化を1回だけ行い、シグネチャの計算とボディの両方に使い回す。
fn stripe_webhook_request(event_id: &str, event_type: &str, object: Value) -> Request<Body> {
    let payload = json!({
        "id": event_id,
        "type": event_type,
        "created": 1_790_000_000,
        "data": { "object": object },
    })
    .to_string();
    let signature = bp_carnet::payments::stripe::sign_webhook_for_test(
        STRIPE_WEBHOOK_SECRET_FOR_TEST,
        payload.as_bytes(),
    );
    Request::post("/api/v1/payments/stripe/webhook")
        .header(header::CONTENT_TYPE, "application/json")
        .header("stripe-signature", signature)
        .body(Body::from(payload))
        .expect("failed to build webhook request")
}

#[sqlx::test]
async fn ocr_status_hides_the_topup_when_stripe_is_not_configured(pool: SqlitePool) {
    insert_user(&pool, "kyoko", "password").await;
    // 使い切っていても、決済が無効なら買い足せない。
    set_ocr_budget(&pool, 100, 100_000).await;
    let app = test_app(pool).await;
    let cookie = login_and_get_cookie(&app, "kyoko", "password").await;

    let response = send(&app, get("/api/v1/ocr/status", Some(&cookie))).await;

    assert_eq!(response.status(), StatusCode::OK);
    let body = json_body(response).await;
    assert_eq!(body["quotas"][0]["remainingPercent"], 0);
    assert_eq!(body["quotaLow"], true);
    assert_eq!(body["topupAvailable"], false);
}

/// 読み取りの枠。既定は無制限 (`null`)、個別の上限があれば各枠の残りの%を返す (docs/ocr.md)。
/// 無料の枠に上限があれば買い足せる。残りが少ないかは、全部の枠を足した残りを買い足し1回分の20%と比べる。
#[sqlx::test]
async fn ocr_status_reports_the_quotas_the_topup_and_whether_the_quota_is_low(pool: SqlitePool) {
    insert_user(&pool, "kyoko", "password").await;
    let user_id = user_id_of(&pool, "kyoko").await;
    let mock = start_stripe_mock().await;
    let app = test_app_with_stripe(pool.clone(), &mock).await;
    let cookie = login_and_get_cookie(&app, "kyoko", "password").await;
    let status = || async {
        let response = send(&app, get("/api/v1/ocr/status", Some(&cookie))).await;
        json_body(response).await
    };

    // 無料枠が無制限 (`test_ocr_costs`) なら、枠を出さない。
    let body = status().await;
    assert_eq!(body["quotas"], Value::Null);
    assert_eq!(body["quotaLow"], false);
    assert_eq!(body["topupAvailable"], false);

    // 買い足し1回分 (60円) の 20% (12円) まで減った無料の枠は少ない。
    set_ocr_budget(&pool, 60, 48_000).await;
    let body = status().await;
    assert_eq!(
        body["quotas"],
        json!([{ "kind": "free", "purchasedAt": null, "remainingPercent": 20 }])
    );
    assert_eq!(body["quotaLow"], true);
    assert_eq!(body["topupAvailable"], true);

    // 少し残った購入の枠と足すと 20% を超える。
    insert_ocr_quota_grant(&pool, user_id, "cs_small", 1000).await;
    let body = status().await;
    assert_eq!(
        body["quotas"],
        json!([
            { "kind": "paid", "purchasedAt": "2026-09-01T00:00:00.000Z", "remainingPercent": 2 },
            { "kind": "free", "purchasedAt": null, "remainingPercent": 20 },
        ])
    );
    assert_eq!(body["quotaLow"], false);
}

/// 買い足した枠を入れる。
async fn insert_ocr_quota_grant(
    pool: &SqlitePool,
    user_id: i64,
    checkout_session_id: &str,
    remaining_milli_yen: i64,
) {
    sqlx::query!(
        "INSERT INTO ocr_quota_grants (user_id, granted_milli_yen, remaining_milli_yen, purchased_at, price_yen, stripe_checkout_session_id) VALUES (?, ?, ?, '2026-09-01T00:00:00.000Z', 300, ?)",
        user_id,
        TEST_TOPUP_GRANT_MILLI_YEN,
        remaining_milli_yen,
        checkout_session_id
    )
    .execute(pool)
    .await
    .expect("failed to insert the grant");
}

/// 管理画面の一覧に、買い足した枠の残り (円、切り上げ) を出す。
#[sqlx::test]
async fn admin_user_list_reports_the_paid_ocr_quota(pool: SqlitePool) {
    insert_admin(&pool, "admin", "password").await;
    insert_user(&pool, "kyoko", "password").await;
    let user_id = user_id_of(&pool, "kyoko").await;
    insert_ocr_quota_grant(&pool, user_id, "cs_first", 1500).await;
    insert_ocr_quota_grant(&pool, user_id, "cs_second", 1000).await;
    // 使い切った枠は残りに入れない。
    insert_ocr_quota_grant(&pool, user_id, "cs_used", 0).await;
    let app = test_app(pool.clone()).await;
    let cookie = login_and_get_cookie(&app, "admin", "password").await;

    let list = send(&app, get("/api/v1/admin/users", Some(&cookie))).await;
    let body = json_body(list).await;
    let users = body["users"].as_array().expect("users should be an array");
    let kyoko = users
        .iter()
        .find(|user| user["username"] == "kyoko")
        .expect("kyoko should be listed");
    // 2.5円は切り上げて3円と出す。
    assert_eq!(kyoko["ocrPaidRemainingYen"], 3);
    let admin = users
        .iter()
        .find(|user| user["username"] == "admin")
        .expect("admin should be listed");
    assert_eq!(admin["ocrPaidRemainingYen"], 0);
}

#[sqlx::test]
async fn ocr_topup_checkout_is_disabled_without_stripe_configuration(pool: SqlitePool) {
    insert_user(&pool, "kyoko", "password").await;
    let app = test_app(pool).await;
    let cookie = login_and_get_cookie(&app, "kyoko", "password").await;

    let response = send(
        &app,
        post_json(
            "/api/v1/payments/ocr-topup/checkout",
            Some(&cookie),
            json!({}),
        ),
    )
    .await;

    assert_error(
        response,
        StatusCode::SERVICE_UNAVAILABLE,
        "payments_disabled",
    )
    .await;
}

/// サーバー設定の Price ID・ログイン中のユーザーだけで Checkout Session を作る
/// (クライアントから金額・Price ID・対象ユーザーを受け取らない)。
#[sqlx::test]
async fn ocr_topup_checkout_creates_a_session_with_the_server_side_price(pool: SqlitePool) {
    insert_user(&pool, "kyoko", "password").await;
    let user_id = user_id_of(&pool, "kyoko").await;
    let mock = start_stripe_mock().await;
    let app = test_app_with_stripe(pool, &mock).await;
    let cookie = login_and_get_cookie(&app, "kyoko", "password").await;

    let response = send(
        &app,
        post_json(
            "/api/v1/payments/ocr-topup/checkout",
            Some(&cookie),
            json!({}),
        ),
    )
    .await;

    assert_eq!(response.status(), StatusCode::OK);
    let body = json_body(response).await;
    assert!(
        body["checkoutUrl"]
            .as_str()
            .expect("checkoutUrl should be a string")
            .starts_with("https://checkout.stripe.test/")
    );

    let sent = mock.last_create_request();
    assert_eq!(sent["mode"], "payment");
    assert_eq!(sent["line_items[0][price]"], "price_test");
    assert_eq!(sent["metadata[user_id]"], user_id.to_string());
    assert_eq!(sent["metadata[product]"], "bp-carnet");
    assert_eq!(sent["managed_payments[enabled]"], "false");
    assert_eq!(
        sent["payment_intent_data[statement_descriptor_suffix]"],
        "BP CARNET"
    );
    assert_eq!(
        sent["payment_method_options[card][statement_descriptor_suffix_kanji]"],
        "BPカルネ"
    );
    assert_eq!(
        sent["payment_method_options[card][statement_descriptor_suffix_kana]"],
        "ビーピーカルネ"
    );
    // 課税事業者になる前は税率を付けない。
    assert!(!sent.contains_key("line_items[0][tax_rates][0]"));
}

/// 課税事業者になった後は、項目に税率を付ける (領収書を適格簡易請求書にするため)。
#[sqlx::test]
async fn ocr_topup_checkout_adds_the_tax_rate_once_taxable(pool: SqlitePool) {
    insert_user(&pool, "kyoko", "password").await;
    let mock = start_stripe_mock().await;
    let stripe =
        stripe_client_for(&mock).with_tax_rate(Some("txr_test"), jiff::Timestamp::UNIX_EPOCH);
    let app = test_app_with_stripe_client(pool, stripe).await;
    let cookie = login_and_get_cookie(&app, "kyoko", "password").await;

    let response = send(
        &app,
        post_json(
            "/api/v1/payments/ocr-topup/checkout",
            Some(&cookie),
            json!({}),
        ),
    )
    .await;

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        mock.last_create_request()["line_items[0][tax_rates][0]"],
        "txr_test"
    );
}

/// 課税事業者になった後に税率が無ければ、Checkout を作らない。
#[sqlx::test]
async fn ocr_topup_checkout_is_refused_without_the_tax_rate_once_taxable(pool: SqlitePool) {
    insert_user(&pool, "kyoko", "password").await;
    let mock = start_stripe_mock().await;
    let stripe = stripe_client_for(&mock).with_tax_rate(None, jiff::Timestamp::UNIX_EPOCH);
    let app = test_app_with_stripe_client(pool, stripe).await;
    let cookie = login_and_get_cookie(&app, "kyoko", "password").await;

    let response = send(
        &app,
        post_json(
            "/api/v1/payments/ocr-topup/checkout",
            Some(&cookie),
            json!({}),
        ),
    )
    .await;

    assert_error(
        response,
        StatusCode::SERVICE_UNAVAILABLE,
        "payments_disabled",
    )
    .await;
    assert!(
        mock.state
            .create_requests
            .lock()
            .expect("mutex lock should not be poisoned")
            .is_empty()
    );
}

#[sqlx::test]
async fn stripe_webhook_rejects_an_invalid_signature(pool: SqlitePool) {
    let mock = start_stripe_mock().await;
    let app = test_app_with_stripe(pool, &mock).await;

    let payload =
        json!({"id": "evt_1", "type": "unknown.event", "data": {"object": {}}}).to_string();
    let request = Request::post("/api/v1/payments/stripe/webhook")
        .header(header::CONTENT_TYPE, "application/json")
        .header("stripe-signature", "t=1,v1=not-a-real-signature")
        .body(Body::from(payload))
        .expect("failed to build webhook request");

    let response = send(&app, request).await;

    assert_error(
        response,
        StatusCode::BAD_REQUEST,
        "stripe_webhook_invalid_signature",
    )
    .await;
}

/// 未知のイベント種別は記録だけして 2xx を返す。
/// 同じ Event ID の再送では、処理済みとして Stripe API を呼び直さない (冪等性)。
#[sqlx::test]
async fn stripe_webhook_is_idempotent_for_the_same_event_id(pool: SqlitePool) {
    let mock = start_stripe_mock().await;
    let app = test_app_with_stripe(pool, &mock).await;

    let request = stripe_webhook_request("evt_unknown_1", "some.future.event", json!({}));
    let response = send(&app, request).await;
    assert_eq!(response.status(), StatusCode::OK);

    let request = stripe_webhook_request("evt_unknown_1", "some.future.event", json!({}));
    let response = send(&app, request).await;
    assert_eq!(response.status(), StatusCode::OK);

    // 未知のイベントは Session を再検証しないため、Stripe (mock) への GET は発生しない。
    assert_eq!(mock.retrieve_request_count(), 0);
}

/// サーバーが作る買い足しの Checkout Session (支払い済み・300円・設定の Price 1件)。
/// `payment_intent` は `id` の `cs_` を `pi_` に替えたもの。テストは違う所だけを書き換える。
fn paid_checkout_session(id: &str, user_id: i64) -> Value {
    json!({
        "id": id,
        "url": Value::Null,
        "status": "complete",
        "payment_status": "paid",
        "mode": "payment",
        "client_reference_id": user_id.to_string(),
        "payment_intent": format!("pi_{}", id.trim_start_matches("cs_")),
        "amount_total": 300,
        "currency": "jpy",
        "line_items": { "data": [{ "price": { "id": "price_test" }, "quantity": 1 }] },
        "metadata": { "user_id": user_id.to_string() },
    })
}

/// `checkout.session.completed` の Webhook を送り、200 で受け取られたことを確かめる。
async fn send_completed(app: &Router, event_id: &str, session: Value) {
    let request = stripe_webhook_request(event_id, "checkout.session.completed", session);
    let response = send(app, request).await;
    assert_eq!(response.status(), StatusCode::OK, "{event_id}");
}

/// `charge.refunded` の Webhook を送り、200 で受け取られたことを確かめる。
async fn send_refunded(app: &Router, event_id: &str, payment_intent: &str) {
    let request = stripe_webhook_request(
        event_id,
        "charge.refunded",
        json!({ "payment_intent": payment_intent }),
    );
    let response = send(app, request).await;
    assert_eq!(response.status(), StatusCode::OK, "{event_id}");
}

/// `/payments/ocr-topup/result` の `status`。
async fn topup_result(app: &Router, cookie: &str, session_id: &str) -> Value {
    let uri = format!("/api/v1/payments/ocr-topup/result?sessionId={session_id}");
    let response = send(app, get(&uri, Some(cookie))).await;
    json_body(response).await["status"].clone()
}

/// `checkout.session.completed` は Session を再検証したうえで枠を付与する。
#[sqlx::test]
async fn stripe_webhook_verifies_and_grants_a_completed_checkout(pool: SqlitePool) {
    insert_user(&pool, "kyoko", "password").await;
    let user_id = user_id_of(&pool, "kyoko").await;
    let mock = start_stripe_mock().await;
    let app = test_app_with_stripe(pool.clone(), &mock).await;
    let cookie = login_and_get_cookie(&app, "kyoko", "password").await;

    let mut session = paid_checkout_session("cs_paid_1", user_id);
    session["metadata"]["product"] = json!("bp-carnet");
    session["customer_details"] = json!({ "address": { "country": "JP" } });
    mock.seed_session(session.clone());

    send_completed(&app, "evt_completed_1", session).await;
    // metadata だけでなく Stripe API から取り直して検証している。
    assert_eq!(mock.retrieve_request_count(), 1);

    // 300円の決済で、設定した買い足し1回分を付与する。
    assert_eq!(
        paid_remaining(&pool, user_id).await,
        TEST_TOPUP_GRANT_MILLI_YEN
    );
    // 住所の国から、国内の買い手として残す (使った額の記録に写すため)。
    assert_eq!(
        grant_domestic(&pool, "stripe_checkout_session_id", "cs_paid_1").await,
        Some(1)
    );
    assert_eq!(topup_result(&app, &cookie, "cs_paid_1").await, "succeeded");
}

/// `charge.refunded` は、対応する Checkout Session (payment_intent で逆引き) の未使用枠を
/// 直ちに 0 にする。失効規則に依存しないため、実装前ゲートの対象外。
#[sqlx::test]
async fn stripe_webhook_revokes_unspent_quota_on_refund(pool: SqlitePool) {
    insert_user(&pool, "kyoko", "password").await;
    let user_id = user_id_of(&pool, "kyoko").await;
    let mock = start_stripe_mock().await;
    let app = test_app_with_stripe(pool.clone(), &mock).await;

    // 返金の検証に必要な枠だけを直接用意する (`checkout.session.completed` のWebhook経由を経なくてよい)。
    let mut tx = pool.begin().await.expect("transaction should start");
    bp_carnet::payments::ocr_quota::grant(
        &mut tx,
        user_id,
        "cs_refunded_1",
        "2026-01-01T00:00:00.000Z",
        TEST_TOPUP_GRANT_MILLI_YEN,
        Some(true),
    )
    .await
    .expect("grant should be inserted");
    tx.commit().await.expect("transaction should commit");
    assert_eq!(
        paid_remaining(&pool, user_id).await,
        TEST_TOPUP_GRANT_MILLI_YEN
    );
    mock.seed_session(paid_checkout_session("cs_refunded_1", user_id));

    send_refunded(&app, "evt_refunded_1", "pi_refunded_1").await;

    assert_eq!(paid_remaining(&pool, user_id).await, 0);
}

/// サーバーが作った買い足しと食い違う Checkout Session は、200 で受け取って付与しない。
/// - 実際の決済額・通貨が想定 (300円) と違う。
/// - line item が2件以上ある (先頭が設定済み Price・数量1と一致していても)。
/// - 同じ Stripe のアカウントのほかの製品の決済。Session を取り直しもしない。
#[sqlx::test]
async fn stripe_webhook_does_not_grant_a_checkout_that_differs_from_the_topup(pool: SqlitePool) {
    insert_user(&pool, "kyoko", "password").await;
    let user_id = user_id_of(&pool, "kyoko").await;
    let mock = start_stripe_mock().await;
    let app = test_app_with_stripe(pool.clone(), &mock).await;

    let mut wrong_amount = paid_checkout_session("cs_wrong_amount", user_id);
    wrong_amount["amount_total"] = json!(3000);
    let mut extra_line_item = paid_checkout_session("cs_extra_line_item", user_id);
    extra_line_item["line_items"]["data"]
        .as_array_mut()
        .expect("line items should be an array")
        .push(json!({ "price": { "id": "price_other" }, "quantity": 1 }));
    let mut other_product = paid_checkout_session("cs_other_product", user_id);
    other_product["client_reference_id"] = Value::Null;
    other_product["amount_total"] = json!(4980);
    other_product["metadata"] = json!({ "product": "other-product" });

    for (case, session, retrieved) in [
        ("unexpected amount", wrong_amount, 1),
        ("extra line item", extra_line_item, 1),
        ("another product", other_product, 0),
    ] {
        let before = mock.retrieve_request_count();
        mock.seed_session(session.clone());
        send_completed(&app, &format!("evt_{case}"), session).await;
        assert_eq!(mock.retrieve_request_count() - before, retrieved, "{case}");
        assert_eq!(paid_remaining(&pool, user_id).await, 0, "{case}");
    }
}

/// `charge.refunded` が `checkout.session.completed` より先に届いても (配送順序の逆転)、
/// 後から届く completed で枠を付与しない。
#[sqlx::test]
async fn stripe_webhook_does_not_grant_a_checkout_already_refunded(pool: SqlitePool) {
    insert_user(&pool, "kyoko", "password").await;
    let user_id = user_id_of(&pool, "kyoko").await;
    let mock = start_stripe_mock().await;
    let app = test_app_with_stripe(pool.clone(), &mock).await;
    let session = paid_checkout_session("cs_refunded_first", user_id);
    mock.seed_session(session.clone());

    send_refunded(&app, "evt_refunded_first", "pi_refunded_first").await;
    send_completed(&app, "evt_completed_after_refund", session).await;

    assert_eq!(paid_remaining(&pool, user_id).await, 0);
}

/// 付与済みの購入が後から返金されたら、`/payments/ocr-topup/result` を成功表示のまま
/// にしない。
#[sqlx::test]
async fn ocr_topup_result_reports_failed_after_a_grant_is_later_refunded(pool: SqlitePool) {
    insert_user(&pool, "kyoko", "password").await;
    let user_id = user_id_of(&pool, "kyoko").await;
    let mock = start_stripe_mock().await;
    let app = test_app_with_stripe(pool.clone(), &mock).await;
    let cookie = login_and_get_cookie(&app, "kyoko", "password").await;
    let session = paid_checkout_session("cs_granted_then_refunded", user_id);
    mock.seed_session(session.clone());

    send_completed(&app, "evt_granted_then_refunded", session).await;
    assert_eq!(
        topup_result(&app, &cookie, "cs_granted_then_refunded").await,
        "succeeded"
    );

    send_refunded(&app, "evt_refund_after_grant", "pi_granted_then_refunded").await;
    assert_eq!(
        topup_result(&app, &cookie, "cs_granted_then_refunded").await,
        "failed"
    );
}

/// コンビニ決済などの非同期手段の失敗は、Checkout Session がすぐには `expired` にならなくても、
/// `/payments/ocr-topup/result` が `processing` のまま止めず `failed` を返す。
#[sqlx::test]
async fn ocr_topup_result_reports_failed_after_async_payment_failed(pool: SqlitePool) {
    insert_user(&pool, "kyoko", "password").await;
    let user_id = user_id_of(&pool, "kyoko").await;
    let mock = start_stripe_mock().await;
    let app = test_app_with_stripe(pool.clone(), &mock).await;
    let cookie = login_and_get_cookie(&app, "kyoko", "password").await;
    let mut session = paid_checkout_session("cs_async_failed", user_id);
    session["status"] = json!("open");
    session["payment_status"] = json!("unpaid");
    mock.seed_session(session.clone());

    let request = stripe_webhook_request(
        "evt_async_failed",
        "checkout.session.async_payment_failed",
        session,
    );
    let response = send(&app, request).await;
    assert_eq!(response.status(), StatusCode::OK);

    assert_eq!(
        topup_result(&app, &cookie, "cs_async_failed").await,
        "failed"
    );
}

/// `charge.refunded` が `checkout.session.completed` より先に届き (配送順序の逆転)、
/// grant がそもそも作られなかった場合でも `processing` のまま止めず `failed` を返す。
#[sqlx::test]
async fn ocr_topup_result_reports_failed_when_refunded_before_a_grant_exists(pool: SqlitePool) {
    insert_user(&pool, "kyoko", "password").await;
    let user_id = user_id_of(&pool, "kyoko").await;
    let mock = start_stripe_mock().await;
    let app = test_app_with_stripe(pool.clone(), &mock).await;
    let cookie = login_and_get_cookie(&app, "kyoko", "password").await;
    mock.seed_session(paid_checkout_session("cs_refunded_before_grant", user_id));

    send_refunded(
        &app,
        "evt_refunded_before_grant",
        "pi_refunded_before_grant",
    )
    .await;

    assert_eq!(
        topup_result(&app, &cookie, "cs_refunded_before_grant").await,
        "failed"
    );
}

#[sqlx::test]
async fn ocr_topup_result_rejects_another_users_session(pool: SqlitePool) {
    insert_user(&pool, "kyoko", "password").await;
    insert_user(&pool, "taro", "password").await;
    let other_user_id = user_id_of(&pool, "taro").await;
    let mock = start_stripe_mock().await;
    let app = test_app_with_stripe(pool, &mock).await;
    let cookie = login_and_get_cookie(&app, "kyoko", "password").await;

    mock.seed_session(json!({
        "id": "cs_other_user",
        "url": Value::Null,
        "status": "open",
        "payment_status": "unpaid",
        "mode": "payment",
        "client_reference_id": other_user_id.to_string(),
        "payment_intent": Value::Null,
        "metadata": { "user_id": other_user_id.to_string() },
    }));

    let response = send(
        &app,
        get(
            "/api/v1/payments/ocr-topup/result?sessionId=cs_other_user",
            Some(&cookie),
        ),
    )
    .await;

    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}
