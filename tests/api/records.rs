//! 記録の CRUD と取り込み。

use super::*;

/// 登録の値の検証が HTTP の入口につながり、共通の envelope で 422 を返すこと。
/// 場合ごとの検証は `src/api/records.rs` の `validate_and_normalize` のテスト。
#[sqlx::test]
async fn create_record_rejects_an_invalid_value(pool: SqlitePool) {
    let (app, cookie) = logged_in_app(pool).await;

    let response = send(
        &app,
        create_record_request(
            &cookie,
            json!({"localMeasuredAt": "2026-09-07T08:30", "systolic": 80, "diastolic": 80}),
        ),
    )
    .await;

    assert_error(
        response,
        StatusCode::UNPROCESSABLE_ENTITY,
        "validation_error",
    )
    .await;
}

/// 登録した記録が一覧に測定日時の新しい順で出てくることを確認する
/// (エンドツーエンドの「記録して見返せる」という最小の価値のスライス)。
#[sqlx::test]
async fn create_record_and_list_roundtrip(pool: SqlitePool) {
    let (app, cookie) = logged_in_app(pool).await;

    let older = json!({
        "localMeasuredAt": "2026-09-01T08:00",
        "systolic": 118,
        "diastolic": 76
    });
    let newer = json!({
        "localMeasuredAt": "2026-09-07T08:00",
        "systolic": 124,
        "diastolic": 82,
        "pulse": 70,
        "memo": "起床直後"
    });

    let create_response = send(&app, create_record_request(&cookie, older)).await;
    assert_eq!(create_response.status(), StatusCode::CREATED);
    let created = json_body(create_response).await;
    assert!(created["id"].is_i64());
    // 日時はユーザーのタイムゾーンのまま返し、UTC の値は返さない。
    assert_eq!(created["localMeasuredAt"], "2026-09-01T08:00");
    assert!(created.get("measuredAt").is_none());

    let create_response = send(&app, create_record_request(&cookie, newer)).await;
    assert_eq!(create_response.status(), StatusCode::CREATED);

    let list_response = send(&app, get("/api/v1/records", Some(&cookie))).await;
    assert_eq!(list_response.status(), StatusCode::OK);
    let records = json_body(list_response).await;
    let records = records.as_array().expect("response should be an array");
    assert_eq!(records.len(), 2);
    // 測定日時の新しい順。
    assert_eq!(records[0]["memo"], "起床直後");
    assert_eq!(records[0]["pulse"], 70);
    assert_eq!(records[1]["localMeasuredAt"], "2026-09-01T08:00");
    assert_eq!(records[1]["pulse"], Value::Null);
}

/// 一覧の各記録に、ユーザーの設定 (既定の Asia/Tokyo・朝4:00-10:00・夜18:00-24:00) で
/// 判定した朝/夜の区分が付くことを確認する。
#[sqlx::test]
async fn list_records_includes_day_period_in_user_timezone(pool: SqlitePool) {
    let (app, cookie) = logged_in_app(pool).await;

    // 朝 7:15 JST は UTC では前日。UTC の時刻で判定すると夜になってしまう。
    for measured_at in ["2026-09-06T07:15", "2026-09-06T12:30", "2026-09-06T21:40"] {
        create_record(
            &app,
            &cookie,
            json!({"localMeasuredAt": measured_at, "systolic": 120, "diastolic": 80}),
        )
        .await;
    }

    let response = send(&app, get("/api/v1/records", Some(&cookie))).await;
    assert_eq!(response.status(), StatusCode::OK);
    let records = json_body(response).await;
    let records = records.as_array().expect("response should be an array");
    let periods: Vec<(&Value, &Value)> = records
        .iter()
        .map(|record| (&record["localMeasuredAt"], &record["dayPeriod"]))
        .collect();
    // 測定日時の新しい順 (21:40・12:30・7:15)。日中は朝/夜どちらでもない。表示用の日時は
    // 判定と同じ Asia/Tokyo のもの。
    assert_eq!(
        periods,
        [
            (&json!("2026-09-06T21:40"), &json!("evening")),
            (&json!("2026-09-06T12:30"), &Value::Null),
            (&json!("2026-09-06T07:15"), &json!("morning")),
        ]
    );
}

/// 取り込みのリクエスト。`body` に `expected` が無ければ、確認画面を出した時点の記録として、
/// 今の置き換える日 (`records` に行がある日) の記録の id と版番号を入れる。
async fn import_records_request(app: &Router, cookie: &str, mut body: Value) -> Request<Body> {
    if body.get("expected").is_none() {
        let days: std::collections::BTreeSet<String> = body["records"]
            .as_array()
            .expect("records should be an array")
            .iter()
            .filter_map(|record| record["localMeasuredAt"].as_str())
            .map(|value| value.chars().take(10).collect())
            .collect();
        let response = send(app, get("/api/v1/records", Some(cookie))).await;
        let records = json_body(response).await;
        let expected: Vec<Value> = records
            .as_array()
            .expect("response should be an array")
            .iter()
            .filter(|record| {
                let date: String = record["localMeasuredAt"]
                    .as_str()
                    .expect("string")
                    .chars()
                    .take(10)
                    .collect();
                days.contains(&date)
            })
            .map(|record| json!({"id": record["id"], "version": record["version"]}))
            .collect();
        body["expected"] = Value::Array(expected);
    }
    post_json("/api/v1/records/import", Some(cookie), body)
}

/// 行の数の上限・下限は、行ごとの検証の前に 422 で断る。
#[sqlx::test]
async fn import_records_rejects_an_empty_or_too_large_batch(pool: SqlitePool) {
    let (app, cookie) = logged_in_app(pool).await;
    let too_many: Vec<Value> = (0..10_001)
        .map(|i| {
            json!({
                "localMeasuredAt": format!("2026-01-{:02}T07:00", (i % 28) + 1),
                "systolic": 120,
                "diastolic": 80
            })
        })
        .collect();

    for (name, records) in [("empty", vec![]), ("more than the maximum", too_many)] {
        let request = import_records_request(
            &app,
            &cookie,
            json!({"from": "2026-01-01", "to": "2026-01-28", "records": records}),
        )
        .await;
        let response = send(&app, request).await;
        assert_error_case(
            response,
            StatusCode::UNPROCESSABLE_ENTITY,
            "validation_error",
            name,
        )
        .await;
    }
}

/// 1件でも不正な行があれば、その行だけでなく全行 (最初の1件で打ち切らず) を検証し、
/// `error.rows` に index/message を列挙して返すこと。`from`〜`to` の範囲外の行も不正な行に数える
/// (確認画面に出した範囲の外にある行を、利用者が見ないまま取り込まないため)。また、有効な行を
/// 含んでいても削除・新規登録のどちらも行われないこと (全件アトミック) を確認する。
#[sqlx::test]
async fn import_records_reports_all_row_errors_without_partial_replace(pool: SqlitePool) {
    let (app, cookie) = logged_in_app(pool).await;

    let response = send(
        &app,
        create_record_request(
            &cookie,
            json!({"localMeasuredAt": "2026-09-03T07:00", "systolic": 130, "diastolic": 85}),
        ),
    )
    .await;
    assert_eq!(response.status(), StatusCode::CREATED);

    let response = send(
        &app,
        import_records_request(
            &app,
            &cookie,
            json!({
                "from": "2026-09-01",
                "to": "2026-09-07",
                "records": [
                    {"localMeasuredAt": "2026-09-05T07:30", "systolic": 128, "diastolic": 82},
                    {"localMeasuredAt": "2026-09-06T07:30", "systolic": 80, "diastolic": 80},
                    {"localMeasuredAt": "not-a-datetime", "systolic": 120, "diastolic": 80},
                    {"localMeasuredAt": "2026-09-10T07:00", "systolic": 125, "diastolic": 78}
                ]
            }),
        )
        .await,
    )
    .await;

    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    let error = json_body(response).await;
    assert_eq!(error["error"]["code"], "batch_validation_error");
    let rows = error["error"]["rows"]
        .as_array()
        .expect("rows should be an array");
    let indexes: Vec<&Value> = rows.iter().map(|row| &row["index"]).collect();
    assert_eq!(
        indexes,
        [&json!(1), &json!(2), &json!(3)],
        "only the invalid rows should be reported"
    );

    let list_response = send(&app, get("/api/v1/records", Some(&cookie))).await;
    let records = json_body(list_response).await;
    assert_eq!(
        records.as_array().expect("array").len(),
        1,
        "既存の1件だけが残り、削除も新規登録も行われないこと"
    );
}

/// 取り込む行がある日だけが置き換わり、行が1件も無い日 (範囲の中でも) と範囲外の既存
/// レコードは残ることを確認する (docs/import-export.md)。取り込みが一括削除の手段にならないための
/// 中核の挙動。
#[sqlx::test]
async fn import_records_replaces_only_days_that_have_rows(pool: SqlitePool) {
    let (app, cookie) = logged_in_app(pool).await;

    // 範囲外 (8/31) に1件、取り込む行と同じ日 (9/1) に1件、範囲内だが取り込む行の無い日
    // (9/3) に1件。
    for body in [
        json!({"localMeasuredAt": "2026-08-31T23:00", "systolic": 110, "diastolic": 70}),
        json!({"localMeasuredAt": "2026-09-01T21:00", "systolic": 132, "diastolic": 86}),
        json!({"localMeasuredAt": "2026-09-03T07:00", "systolic": 130, "diastolic": 85}),
    ] {
        let response = send(&app, create_record_request(&cookie, body)).await;
        assert_eq!(response.status(), StatusCode::CREATED);
    }

    let response = send(
        &app,
        import_records_request(
            &app,
            &cookie,
            json!({
                "from": "2026-09-01",
                "to": "2026-09-07",
                "records": [
                    {"localMeasuredAt": "2026-09-01T07:00", "systolic": 125, "diastolic": 78}
                ]
            }),
        )
        .await,
    )
    .await;

    assert_eq!(response.status(), StatusCode::OK);
    let result = json_body(response).await;
    assert_eq!(result["deleted"], 1, "置き換えた 9/1 の既存1件だけ");
    assert_eq!(result["created"].as_array().expect("array").len(), 1);

    let list_response = send(&app, get("/api/v1/records", Some(&cookie))).await;
    let records = json_body(list_response).await;
    let records = records.as_array().expect("array");
    let measured: Vec<&str> = records
        .iter()
        .map(|r| r["localMeasuredAt"].as_str().expect("string"))
        .collect();
    assert!(
        measured.contains(&"2026-08-31T23:00"),
        "範囲外の既存レコードは削除されない: {measured:?}"
    );
    assert!(
        measured.contains(&"2026-09-03T07:00"),
        "範囲内でも取り込む行が無い日の既存レコードは削除されない: {measured:?}"
    );
    assert!(
        !measured.contains(&"2026-09-01T21:00"),
        "取り込む行がある日の既存レコードは置き換えで消える: {measured:?}"
    );
    assert!(
        measured.contains(&"2026-09-01T07:00"),
        "取り込んだ行が入っている: {measured:?}"
    );
    assert_eq!(measured.len(), 3, "8/31・9/3 の既存 + 取り込んだ1件");
}

/// 同じ内容を2回取り込んでも結果が変わらないこと (docs/import-export.md) を確認する。
#[sqlx::test]
async fn import_records_is_idempotent(pool: SqlitePool) {
    let (app, cookie) = logged_in_app(pool).await;

    let body = json!({
        "from": "2026-09-01",
        "to": "2026-09-07",
        "records": [
            {"localMeasuredAt": "2026-09-01T07:00", "systolic": 125, "diastolic": 78, "pulse": 70},
            {"localMeasuredAt": "2026-09-05T21:00", "systolic": 118, "diastolic": 76}
        ]
    });

    for _ in 0..2 {
        let response = send(
            &app,
            import_records_request(&app, &cookie, body.clone()).await,
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);
    }

    let list_response = send(&app, get("/api/v1/records", Some(&cookie))).await;
    let records = json_body(list_response).await;
    assert_eq!(
        records.as_array().expect("array").len(),
        2,
        "2回目の取り込みで重複登録されないこと"
    );
}

/// 対象日が複数あるとき、`deleted` が各日の削除件数の合計になることを確認する
/// (日ごとに `DELETE` を回すため、合算を取りこぼさないこと)。
#[sqlx::test]
async fn import_records_counts_deletions_across_days(pool: SqlitePool) {
    let (app, cookie) = logged_in_app(pool).await;

    // 置き換える2日 (9/1・9/5) にそれぞれ既存1件、置き換えない日 (9/3) にも1件。
    for body in [
        json!({"localMeasuredAt": "2026-09-01T21:00", "systolic": 132, "diastolic": 86}),
        json!({"localMeasuredAt": "2026-09-03T07:00", "systolic": 130, "diastolic": 85}),
        json!({"localMeasuredAt": "2026-09-05T21:00", "systolic": 128, "diastolic": 80}),
    ] {
        let response = send(&app, create_record_request(&cookie, body)).await;
        assert_eq!(response.status(), StatusCode::CREATED);
    }

    let response = send(
        &app,
        import_records_request(
            &app,
            &cookie,
            json!({
                "from": "2026-09-01",
                "to": "2026-09-07",
                "records": [
                    {"localMeasuredAt": "2026-09-01T07:00", "systolic": 125, "diastolic": 78},
                    {"localMeasuredAt": "2026-09-05T07:00", "systolic": 122, "diastolic": 74}
                ]
            }),
        )
        .await,
    )
    .await;

    assert_eq!(response.status(), StatusCode::OK);
    let result = json_body(response).await;
    assert_eq!(result["deleted"], 2, "9/1 と 9/5 の既存1件ずつの合計");

    let list_response = send(&app, get("/api/v1/records", Some(&cookie))).await;
    let records = json_body(list_response).await;
    let measured: Vec<&str> = records
        .as_array()
        .expect("array")
        .iter()
        .map(|r| r["localMeasuredAt"].as_str().expect("string"))
        .collect();
    assert!(
        measured.contains(&"2026-09-03T07:00"),
        "置き換えない日の記録は残る: {measured:?}"
    );
    assert_eq!(measured.len(), 3, "9/3 の既存 + 取り込んだ2件");
}

/// 他ユーザーの記録は、置き換える日そのものにあっても削除されないこと (データ完全分離) を
/// 確認する。bob の記録を alice が置き換える日に置くことで、`DELETE` の `user_id` 条件が
/// 効いていることを確かめる (別の日に置くと、対象日でないだけで消えず、検証にならない)。
#[sqlx::test]
async fn import_records_does_not_touch_other_users_records(pool: SqlitePool) {
    insert_user(&pool, "alice", "password-a").await;
    insert_user(&pool, "bob", "password-b").await;
    let app = test_app(pool).await;

    let alice_cookie = login_and_get_cookie(&app, "alice", "password-a").await;
    let bob_cookie = login_and_get_cookie(&app, "bob", "password-b").await;

    let response = send(
        &app,
        create_record_request(
            &bob_cookie,
            json!({"localMeasuredAt": "2026-09-01T21:00", "systolic": 130, "diastolic": 85}),
        ),
    )
    .await;
    assert_eq!(response.status(), StatusCode::CREATED);

    let response = send(
        &app,
        import_records_request(
            &app,
            &alice_cookie,
            json!({
                "from": "2026-09-01",
                "to": "2026-09-07",
                "records": [
                    {"localMeasuredAt": "2026-09-01T07:00", "systolic": 125, "diastolic": 78}
                ]
            }),
        )
        .await,
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);

    let bob_records = send(&app, get("/api/v1/records", Some(&bob_cookie))).await;
    let bob_records = json_body(bob_records).await;
    assert_eq!(
        bob_records.as_array().expect("array").len(),
        1,
        "bob の記録は、alice が同じ日を置き換えても削除されない"
    );
}

/// ユーザーごとにデータが完全分離されていること (他ユーザーの記録は一覧に出てこないこと) を
/// 確認する。
#[sqlx::test]
async fn list_records_only_returns_own_users_records(pool: SqlitePool) {
    insert_user(&pool, "alice", "password-a").await;
    insert_user(&pool, "bob", "password-b").await;
    let app = test_app(pool).await;

    let alice_cookie = login_and_get_cookie(&app, "alice", "password-a").await;
    let bob_cookie = login_and_get_cookie(&app, "bob", "password-b").await;

    let alice_create = send(
        &app,
        create_record_request(&alice_cookie, valid_record_body()),
    )
    .await;
    assert_eq!(alice_create.status(), StatusCode::CREATED);

    let bob_list = send(&app, get("/api/v1/records", Some(&bob_cookie))).await;
    assert_eq!(bob_list.status(), StatusCode::OK);
    let bob_records = json_body(bob_list).await;
    assert_eq!(
        bob_records.as_array().expect("should be an array").len(),
        0,
        "bob should not see alice's records"
    );
}

/// `valid_record_body()` に、直すときの版番号を足したもの。
fn versioned_record_body(version: i64) -> Value {
    let mut body = valid_record_body();
    body["version"] = json!(version);
    body
}

/// 更新時も登録時と同じバリデーションが効くことを確認する
/// (値域チェックそのものの網羅は `src/api/records.rs` の `validate_and_normalize_rejects_each_invalid_value` に譲る)。
#[sqlx::test]
async fn update_record_rejects_out_of_range_values(pool: SqlitePool) {
    let (app, cookie) = logged_in_app(pool).await;
    let id = create_record(&app, &cookie, valid_record_body()).await;

    let invalid = json!({"localMeasuredAt": "2026-09-07T08:30", "systolic": 80, "diastolic": 80, "version": 1});
    let response = send(&app, put_json(&record_uri(id), Some(&cookie), invalid)).await;

    assert_error(
        response,
        StatusCode::UNPROCESSABLE_ENTITY,
        "validation_error",
    )
    .await;
}

/// `id` が数値として解釈できない場合、axum の素の rejection (`text/plain`) ではなく
/// 共通 envelope (`AppPath` 経由) で 400 が返ることを確認する。
#[sqlx::test]
async fn record_apis_return_json_envelope_for_non_numeric_id(pool: SqlitePool) {
    let (app, cookie) = logged_in_app(pool).await;

    for (name, request) in [
        (
            "PUT",
            put_json(
                "/api/v1/records/not-a-number",
                Some(&cookie),
                valid_record_body(),
            ),
        ),
        (
            "DELETE",
            delete("/api/v1/records/not-a-number", Some(&cookie)),
        ),
    ] {
        let response = send(&app, request).await;
        assert_error_case(response, StatusCode::BAD_REQUEST, "invalid_path", name).await;
    }
}

/// 無い記録と他ユーザーの記録は、直す・消すのどちらも同じ 404 にする (存在有無を教えないため)。
/// 他ユーザーの記録は変わらず残ること (データ分離) も確かめる。
#[sqlx::test]
async fn record_apis_return_404_for_missing_or_other_users_records(pool: SqlitePool) {
    insert_user(&pool, "alice", "password-a").await;
    insert_user(&pool, "bob", "password-b").await;
    let app = test_app(pool).await;
    let alice_cookie = login_and_get_cookie(&app, "alice", "password-a").await;
    let bob_cookie = login_and_get_cookie(&app, "bob", "password-b").await;
    let alice_record_id = create_record(&app, &alice_cookie, valid_record_body()).await;

    for (name, request) in [
        (
            "PUT a missing record",
            put_json(
                &record_uri(999),
                Some(&bob_cookie),
                versioned_record_body(1),
            ),
        ),
        (
            "DELETE a missing record",
            delete(&format!("{}?version=1", record_uri(999)), Some(&bob_cookie)),
        ),
        (
            "PUT another user's record",
            put_json(
                &record_uri(alice_record_id),
                Some(&bob_cookie),
                versioned_record_body(1),
            ),
        ),
        (
            "DELETE another user's record",
            delete(
                &format!("{}?version=1", record_uri(alice_record_id)),
                Some(&bob_cookie),
            ),
        ),
    ] {
        let response = send(&app, request).await;
        assert_eq!(response.status(), StatusCode::NOT_FOUND, "{name}");
    }

    let response = send(&app, get(&record_uri(alice_record_id), Some(&alice_cookie))).await;
    assert_eq!(response.status(), StatusCode::OK);
    let record = json_body(response).await;
    assert_eq!(record["version"], 1, "alice の記録は直されていない");
    assert_eq!(record["systolic"], valid_record_body()["systolic"]);
}

/// 更新した内容が一覧に反映されることを確認する
/// (エンドツーエンドの「記録を直せる」という価値のスライス)。
#[sqlx::test]
async fn update_record_and_list_roundtrip(pool: SqlitePool) {
    let (app, cookie) = logged_in_app(pool).await;
    let id = create_record(&app, &cookie, valid_record_body()).await;

    let updated = json!({
        "localMeasuredAt": "2026-09-08T07:00",
        "systolic": 130,
        "diastolic": 85,
        "pulse": 72,
        "memo": "夕食後",
        "version": 1
    });
    let update_response = send(&app, put_json(&record_uri(id), Some(&cookie), updated)).await;
    assert_eq!(update_response.status(), StatusCode::OK);
    let body = json_body(update_response).await;
    assert_eq!(body["id"], id);
    assert_eq!(body["localMeasuredAt"], "2026-09-08T07:00");
    assert_eq!(body["systolic"], 130);
    assert_eq!(body["memo"], "夕食後");
    assert_eq!(body["version"], 2, "直すたびに版番号が増える");

    let list_response = send(&app, get("/api/v1/records", Some(&cookie))).await;
    let records = json_body(list_response).await;
    let records = records.as_array().expect("response should be an array");
    assert_eq!(records.len(), 1);
    assert_eq!(records[0]["systolic"], 130);
    assert_eq!(records[0]["memo"], "夕食後");
}

/// 開いた時点の版番号で直すと、同じ版番号での2回目は、ほかで変わったとして 409 で止まり、
/// 記録は1回目の内容のまま残ることを確認する。
#[sqlx::test]
async fn update_record_with_stale_version_returns_409(pool: SqlitePool) {
    let (app, cookie) = logged_in_app(pool).await;
    let id = create_record(&app, &cookie, valid_record_body()).await;

    let mut first = versioned_record_body(1);
    first["systolic"] = json!(130);
    let response = send(&app, put_json(&record_uri(id), Some(&cookie), first)).await;
    assert_eq!(response.status(), StatusCode::OK);

    let response = send(
        &app,
        put_json(&record_uri(id), Some(&cookie), versioned_record_body(1)),
    )
    .await;
    assert_error(response, StatusCode::CONFLICT, "record_conflict").await;

    let response = send(&app, get(&record_uri(id), Some(&cookie))).await;
    assert_eq!(response.status(), StatusCode::OK);
    let body = json_body(response).await;
    assert_eq!(body["systolic"], 130);
    assert_eq!(body["version"], 2);
}

/// 版番号が古いと削除せず 409、記録がもう無ければ 404 になることを確認する。
#[sqlx::test]
async fn delete_record_with_stale_version_returns_409(pool: SqlitePool) {
    let (app, cookie) = logged_in_app(pool).await;
    let id = create_record(&app, &cookie, valid_record_body()).await;
    let response = send(
        &app,
        put_json(&record_uri(id), Some(&cookie), versioned_record_body(1)),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);

    let response = send(
        &app,
        delete(&format!("{}?version=1", record_uri(id)), Some(&cookie)),
    )
    .await;
    assert_error(response, StatusCode::CONFLICT, "record_conflict").await;

    let response = send(
        &app,
        delete(&format!("{}?version=2", record_uri(id)), Some(&cookie)),
    )
    .await;
    assert_eq!(response.status(), StatusCode::NO_CONTENT);

    let response = send(
        &app,
        delete(&format!("{}?version=2", record_uri(id)), Some(&cookie)),
    )
    .await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

/// 確認画面を出した後に、置き換える日の記録がほかで足されていたら、取り込まずに 409 で止めることを確認する
/// (docs/import-export.md)。
#[sqlx::test]
async fn import_records_returns_409_when_target_day_changed(pool: SqlitePool) {
    let (app, cookie) = logged_in_app(pool).await;
    create_record(
        &app,
        &cookie,
        json!({"localMeasuredAt": "2026-09-01T21:00", "systolic": 132, "diastolic": 86}),
    )
    .await;

    let body = json!({
        "from": "2026-09-01",
        "to": "2026-09-01",
        "records": [{"localMeasuredAt": "2026-09-01T07:00", "systolic": 125, "diastolic": 78}]
    });
    // 確認画面を出した時点の記録で組み立ててから、ほかで同じ日に1件足す。
    let request = import_records_request(&app, &cookie, body).await;
    create_record(
        &app,
        &cookie,
        json!({"localMeasuredAt": "2026-09-01T12:00", "systolic": 120, "diastolic": 80}),
    )
    .await;

    let response = send(&app, request).await;
    assert_error(response, StatusCode::CONFLICT, "record_conflict").await;

    let response = send(&app, get("/api/v1/records", Some(&cookie))).await;
    assert_eq!(
        json_body(response).await.as_array().expect("array").len(),
        2,
        "何も変えない"
    );
}

/// 削除した記録が一覧から消えることを確認する
/// (エンドツーエンドの「記録を消せる」という価値のスライス)。
#[sqlx::test]
async fn delete_record_removes_from_list(pool: SqlitePool) {
    let (app, cookie) = logged_in_app(pool).await;
    let id = create_record(&app, &cookie, valid_record_body()).await;

    let delete_response = send(
        &app,
        delete(&format!("{}?version=1", record_uri(id)), Some(&cookie)),
    )
    .await;
    assert_eq!(delete_response.status(), StatusCode::NO_CONTENT);

    let list_response = send(&app, get("/api/v1/records", Some(&cookie))).await;
    let records = json_body(list_response).await;
    assert_eq!(records.as_array().expect("should be an array").len(), 0);
}

/// `from`/`to` の日は丸1日含み、前日 23:59・翌日 0:00 の記録は含まないことを確認する。
#[sqlx::test]
async fn list_records_from_and_to_cover_whole_days(pool: SqlitePool) {
    let (app, cookie) = logged_in_app(pool).await;

    for local_measured_at in [
        "2026-09-04T23:59",
        "2026-09-05T00:00",
        "2026-09-05T23:59",
        "2026-09-06T00:00",
    ] {
        create_record(
            &app,
            &cookie,
            json!({"localMeasuredAt": local_measured_at, "systolic": 120, "diastolic": 80}),
        )
        .await;
    }

    let list_response = send(
        &app,
        get(
            "/api/v1/records?from=2026-09-05&to=2026-09-05",
            Some(&cookie),
        ),
    )
    .await;
    assert_eq!(list_response.status(), StatusCode::OK);
    let records = json_body(list_response).await;
    let local_measured_ats: Vec<&Value> = records
        .as_array()
        .expect("should be an array")
        .iter()
        .map(|record| &record["localMeasuredAt"])
        .collect();
    assert_eq!(
        local_measured_ats,
        [&json!("2026-09-05T23:59"), &json!("2026-09-05T00:00")]
    );
}

/// from が to より後の場合に 422 を返すことを確認する。
#[sqlx::test]
async fn list_records_rejects_from_after_to(pool: SqlitePool) {
    let (app, cookie) = logged_in_app(pool).await;

    let response = send(
        &app,
        get(
            "/api/v1/records?from=2026-09-10&to=2026-09-01",
            Some(&cookie),
        ),
    )
    .await;

    assert_error(
        response,
        StatusCode::UNPROCESSABLE_ENTITY,
        "validation_error",
    )
    .await;
}
