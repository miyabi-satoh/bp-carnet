//! 集計と最新の記録の日付。

use super::*;

/// 朝/夜の集計が、UTC 保存の記録をユーザーのローカル時刻で振り分けて返すことを確認する。
/// 既定 (Asia/Tokyo・朝4:00-10:00・夜18:00-24:00) のまま、設定 UI 無しで成立することも兼ねる。
/// 振り分け・平均の場合ごとの確かめは `src/stats.rs` のテスト。ここでは、記録が無い区分が
/// null で返ること (frontend が「データ無し」を判別する手段) も確かめる。
#[sqlx::test]
async fn summarize_records_groups_by_morning_and_evening(pool: SqlitePool) {
    let (app, cookie) = logged_in_app(pool).await;

    for (measured_at, systolic, diastolic) in [
        // 9/8 の朝2件 (平均 130/85)。
        ("2026-09-08T08:00", 120, 80),
        ("2026-09-08T09:00", 140, 90),
        // 9/8 の夜1件。
        ("2026-09-08T21:00", 128, 82),
        // 12:00 JST は朝でも夜でもないので集計対象外。
        ("2026-09-08T12:00", 200, 100),
        // 9/9 は朝だけ。
        ("2026-09-09T08:00", 130, 85),
    ] {
        create_record(
            &app,
            &cookie,
            json!({"localMeasuredAt": measured_at, "systolic": systolic, "diastolic": diastolic}),
        )
        .await;
    }

    let response = send(&app, get("/api/v1/records/summary", Some(&cookie))).await;
    assert_eq!(response.status(), StatusCode::OK);

    let summary = json_body(response).await;
    assert_eq!(summary["morning"]["systolic"], 130.0);
    assert_eq!(summary["morning"]["diastolic"], 85.0);
    assert_eq!(summary["evening"]["systolic"], 128.0);

    let days = summary["days"].as_array().expect("days should be an array");
    assert_eq!(days.len(), 2);
    assert_eq!(days[0]["date"], "2026-09-08");
    assert_eq!(days[0]["morning"]["systolic"], 130.0);
    assert_eq!(days[0]["evening"]["systolic"], 128.0);
    assert_eq!(days[1]["date"], "2026-09-09");
    assert!(days[1]["evening"].is_null());
}

/// 最新の記録の日が、測定日時の最も新しい記録のユーザーのタイムゾーンでの日付で返り、
/// 朝夜のどちらでもない時刻の記録も数え、他のユーザーの記録は数えないことを確認する。
#[sqlx::test]
async fn latest_record_date_is_the_local_date_of_the_newest_own_record(pool: SqlitePool) {
    insert_user(&pool, "admin", "password").await;
    insert_user(&pool, "other", "password").await;
    // 未来の記録 (明日以降を拒むようになる前の誤入力など) は数えない。API では作れないので直接入れる。
    insert_record_at(&pool, "admin", "2099-01-01T00:00:00Z", 120).await;
    let app = test_app(pool).await;
    let cookie = login_and_get_cookie(&app, "admin", "password").await;
    let other_cookie = login_and_get_cookie(&app, "other", "password").await;

    for (measured_at, cookie) in [
        ("2026-09-08T21:00", &cookie),
        // 日本時間の 9/9 0:30 は UTC では 9/8 15:30。UTC の日付ではなく、ローカルの日付で返す。
        // 朝夜のどちらでもない時刻でも数える。
        ("2026-09-09T00:30", &cookie),
        // 他のユーザーの新しい記録は数えない。
        ("2026-09-20T08:00", &other_cookie),
    ] {
        create_record(
            &app,
            cookie,
            json!({"localMeasuredAt": measured_at, "systolic": 120, "diastolic": 80}),
        )
        .await;
    }

    let response = send(&app, get("/api/v1/records/latest-date", Some(&cookie))).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(json_body(response).await["date"], "2026-09-09");
}

/// 記録が1件も無ければ null を返す (frontend は今の週を出す)。
#[sqlx::test]
async fn latest_record_date_is_null_without_records(pool: SqlitePool) {
    let (app, cookie) = logged_in_app(pool).await;

    let response = send(&app, get("/api/v1/records/latest-date", Some(&cookie))).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert!(json_body(response).await["date"].is_null());
}

#[sqlx::test]
async fn summarize_records_respects_the_from_and_to_range(pool: SqlitePool) {
    let (app, cookie) = logged_in_app(pool).await;
    for measured_at in ["2026-09-01T08:00", "2026-09-05T08:00", "2026-09-10T08:00"] {
        create_record(
            &app,
            &cookie,
            json!({"localMeasuredAt": measured_at, "systolic": 120, "diastolic": 80}),
        )
        .await;
    }

    let response = send(
        &app,
        get(
            "/api/v1/records/summary?from=2026-09-03&to=2026-09-07",
            Some(&cookie),
        ),
    )
    .await;

    let summary = json_body(response).await;
    let days = summary["days"].as_array().expect("days should be an array");
    assert_eq!(days.len(), 1);
    assert_eq!(days[0]["date"], "2026-09-05");
}

/// 他ユーザーの記録が混ざらないことを確認する (所有者チェック)。
#[sqlx::test]
async fn summarize_records_only_covers_own_records(pool: SqlitePool) {
    insert_user(&pool, "admin", "password").await;
    insert_user(&pool, "other", "password").await;
    let app = test_app(pool).await;

    let other_cookie = login_and_get_cookie(&app, "other", "password").await;
    create_record(
        &app,
        &other_cookie,
        json!({"localMeasuredAt": "2026-09-08T08:00", "systolic": 200, "diastolic": 100}),
    )
    .await;

    let cookie = login_and_get_cookie(&app, "admin", "password").await;
    let response = send(&app, get("/api/v1/records/summary", Some(&cookie))).await;

    let summary = json_body(response).await;
    assert!(summary["morning"].is_null());
    assert!(summary["evening"].is_null());
    assert_eq!(
        summary["days"]
            .as_array()
            .expect("days should be an array")
            .len(),
        0
    );
}

/// DB のタイムゾーンが tzdb に無い値だった場合に、500 + code: "unknown_timezone" の
/// envelope になることを確認する (frontend が表示文言を引くための契約)。`/auth/me` も、
/// 使えないタイムゾーンを frontend に渡さない。
#[sqlx::test]
async fn apis_return_500_for_an_unknown_timezone(pool: SqlitePool) {
    insert_user(&pool, "admin", "password").await;
    sqlx::query!("UPDATE users SET timezone = 'Not/AZone'")
        .execute(&pool)
        .await
        .expect("failed to set an unknown timezone");
    let app = test_app(pool).await;
    let cookie = login_and_get_cookie(&app, "admin", "password").await;

    for uri in ["/api/v1/records/summary", "/api/v1/auth/me"] {
        let response = send(&app, get(uri, Some(&cookie))).await;
        assert_error_case(
            response,
            StatusCode::INTERNAL_SERVER_ERROR,
            "unknown_timezone",
            uri,
        )
        .await;
    }
}

#[sqlx::test]
async fn summarize_records_rejects_from_after_to(pool: SqlitePool) {
    let (app, cookie) = logged_in_app(pool).await;

    let response = send(
        &app,
        get(
            "/api/v1/records/summary?from=2026-09-07&to=2026-09-01",
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
