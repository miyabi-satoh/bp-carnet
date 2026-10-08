//! 設定とタイムゾーン。

use super::*;

/// 個人設定の初期値が、マイグレーションの既定値 (Asia/Tokyo・朝4:00-10:00・夜18:00-24:00)
/// として読めることを確認する。
#[sqlx::test]
async fn get_settings_returns_defaults_for_a_new_user(pool: SqlitePool) {
    let (app, cookie) = logged_in_app(pool).await;

    let response = send(&app, get("/api/v1/settings", Some(&cookie))).await;
    assert_eq!(response.status(), StatusCode::OK);

    let settings = json_body(response).await;
    assert_eq!(settings["timezone"], "Asia/Tokyo");
    assert_eq!(settings["timezoneAuto"], true);
    assert_eq!(settings["morningStartMin"], 240);
    assert_eq!(settings["morningEndMin"], 600);
    assert_eq!(settings["eveningStartMin"], 1080);
    assert_eq!(settings["eveningEndMin"], 1440);
}

/// 更新した設定が読み戻せること、および朝/夜の集計が新しい時間帯で行われることを確認する。
/// 設定と集計は別のエンドポイントなので、保存できても集計に効かない状態を検出できるようにする。
#[sqlx::test]
async fn updated_settings_are_persisted_and_applied_to_the_summary(pool: SqlitePool) {
    let (app, cookie) = logged_in_app(pool).await;

    // 既定では朝 (8:00 JST) に入る記録。
    create_record(
        &app,
        &cookie,
        json!({"localMeasuredAt": "2026-09-08T08:00", "systolic": 120, "diastolic": 80}),
    )
    .await;

    // 朝を 5:00-7:00 に狭めると、8:00 の記録はどちらの区分にも入らなくなる。
    let response = send(
        &app,
        put_json(
            "/api/v1/settings",
            Some(&cookie),
            json!({
                "timezone": "Asia/Tokyo",
                "timezoneAuto": false,
                "morningStartMin": 300,
                "morningEndMin": 420,
                "eveningStartMin": 1080,
                "eveningEndMin": 1440
            }),
        ),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(json_body(response).await["morningEndMin"], 420);

    let response = send(&app, get("/api/v1/settings", Some(&cookie))).await;
    assert_eq!(json_body(response).await["morningStartMin"], 300);

    let response = send(&app, get("/api/v1/records/summary", Some(&cookie))).await;
    let summary = json_body(response).await;
    assert!(summary["morning"].is_null());
    assert_eq!(
        summary["days"]
            .as_array()
            .expect("days should be an array")
            .len(),
        0
    );
}

/// タイムゾーンを変えると、同じ UTC の記録が別のローカル日・別の区分に振り分けられる。
#[sqlx::test]
async fn changing_the_timezone_changes_how_records_are_classified(pool: SqlitePool) {
    let (app, cookie) = logged_in_app(pool).await;

    // 9/9 08:00 JST (朝) = 2026-09-08 23:00 UTC = 9/8 19:00 New York (夜)。
    create_record(
        &app,
        &cookie,
        json!({"localMeasuredAt": "2026-09-09T08:00", "systolic": 120, "diastolic": 80}),
    )
    .await;

    let response = send(
        &app,
        put_json(
            "/api/v1/settings",
            Some(&cookie),
            json!({
                "timezone": "America/New_York",
                "timezoneAuto": false,
                "morningStartMin": 240,
                "morningEndMin": 600,
                "eveningStartMin": 1080,
                "eveningEndMin": 1440
            }),
        ),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);

    let response = send(&app, get("/api/v1/records/summary", Some(&cookie))).await;
    let summary = json_body(response).await;
    assert!(summary["morning"].is_null());
    assert_eq!(summary["evening"]["systolic"], 120.0);
    assert_eq!(summary["days"][0]["date"], "2026-09-08");
}

/// 日時の受け渡し・期間の区切り・CSV の書き出しが、個人設定のタイムゾーンで行われることを確認する。
#[sqlx::test]
async fn records_are_read_and_written_in_the_user_timezone(pool: SqlitePool) {
    let (app, cookie) = logged_in_app(pool).await;

    let response = send(
        &app,
        put_json(
            "/api/v1/settings",
            Some(&cookie),
            json!({
                "timezone": "America/New_York",
                "timezoneAuto": false,
                "morningStartMin": 240,
                "morningEndMin": 600,
                "eveningStartMin": 1080,
                "eveningEndMin": 1440
            }),
        ),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);

    let response = send(
        &app,
        create_record_request(
            &cookie,
            json!({"localMeasuredAt": "2026-01-15T07:15", "systolic": 120, "diastolic": 80}),
        ),
    )
    .await;
    assert_eq!(response.status(), StatusCode::CREATED);
    assert_eq!(
        json_body(response).await["localMeasuredAt"],
        "2026-01-15T07:15"
    );

    // 期間の区切りを確かめるため、Asia/Tokyo と New York で日付が変わる境界の記録を足す。
    // 1/15 の範囲は New York なら 1/15 05:00〜1/16 05:00 UTC、Asia/Tokyo なら 1/14 15:00〜1/15 15:00 UTC。
    // - 1/14 20:00 EST = 1/15 01:00 UTC: Asia/Tokyo の 1/15 には入るが、New York では 1/14
    // - 1/15 22:00 EST = 1/16 03:00 UTC: New York の 1/15 には入るが、Asia/Tokyo では 1/16
    for local_measured_at in ["2026-01-14T20:00", "2026-01-15T22:00"] {
        create_record(
            &app,
            &cookie,
            json!({"localMeasuredAt": local_measured_at, "systolic": 120, "diastolic": 80}),
        )
        .await;
    }

    let response = send(
        &app,
        get(
            "/api/v1/records?from=2026-01-15&to=2026-01-15",
            Some(&cookie),
        ),
    )
    .await;
    let records = json_body(response).await;
    let local_measured_ats: Vec<_> = records
        .as_array()
        .expect("should be an array")
        .iter()
        .map(|r| r["localMeasuredAt"].as_str())
        .collect();
    assert_eq!(
        local_measured_ats,
        vec![Some("2026-01-15T22:00"), Some("2026-01-15T07:15")]
    );

    // 書き出しも設定のタイムゾーン (EST) の時刻。01:00 UTC は前日の 20:00 EST。
    let response = send(&app, get("/api/v1/records/export", Some(&cookie))).await;
    let records = export_csv_rows(response).await;
    let measured_ats: Vec<_> = records.iter().map(|r| r["measuredAt"].as_str()).collect();
    assert_eq!(
        measured_ats,
        vec!["2026-01-14 20:00", "2026-01-15 07:15", "2026-01-15 22:00"]
    );
}

/// 設定の検証が、それぞれの API の入口につながっていることを確かめる。タイムゾーン名・時間帯の
/// 場合ごとの検証は `src/validation.rs` のテスト。
#[sqlx::test]
async fn settings_apis_reject_invalid_values(pool: SqlitePool) {
    let (app, cookie) = logged_in_app(pool).await;
    let settings = |timezone: &str, morning_end_min: i64| {
        json!({
            "timezone": timezone,
            "timezoneAuto": false,
            "morningStartMin": 240,
            "morningEndMin": morning_end_min,
            "eveningStartMin": 1080,
            "eveningEndMin": 1440
        })
    };

    for (name, uri, body) in [
        (
            "unknown timezone",
            "/api/v1/settings",
            settings("Not/AZone", 600),
        ),
        // 朝と夜が重なると、1つの記録が朝と夜の両方に数えられる。
        (
            "overlapping periods",
            "/api/v1/settings",
            settings("Asia/Tokyo", 1140),
        ),
        (
            "unknown detected timezone",
            "/api/v1/settings/detected-timezone",
            json!({"timezone": "Not/AZone"}),
        ),
    ] {
        let response = send(&app, put_json(uri, Some(&cookie), body)).await;
        assert_error_case(
            response,
            StatusCode::UNPROCESSABLE_ENTITY,
            "validation_error",
            name,
        )
        .await;
    }

    let response = send(&app, get("/api/v1/settings", Some(&cookie))).await;
    let saved = json_body(response).await;
    assert_eq!(saved["timezone"], "Asia/Tokyo", "拒んだ値は保存しない");
    assert_eq!(saved["morningEndMin"], 600, "拒んだ値は保存しない");
}

/// 設定はセッションのユーザーにのみ適用される (他ユーザーの設定を書き換えない)。
#[sqlx::test]
async fn update_settings_does_not_affect_other_users(pool: SqlitePool) {
    insert_user(&pool, "admin", "password").await;
    insert_user(&pool, "other", "password").await;
    let app = test_app(pool).await;
    let admin_cookie = login_and_get_cookie(&app, "admin", "password").await;
    let other_cookie = login_and_get_cookie(&app, "other", "password").await;

    let response = send(
        &app,
        put_json(
            "/api/v1/settings",
            Some(&admin_cookie),
            json!({
                "timezone": "America/New_York",
                "timezoneAuto": false,
                "morningStartMin": 300,
                "morningEndMin": 420,
                "eveningStartMin": 1080,
                "eveningEndMin": 1440
            }),
        ),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);

    let response = send(&app, get("/api/v1/settings", Some(&other_cookie))).await;
    let settings = json_body(response).await;
    assert_eq!(settings["timezone"], "Asia/Tokyo");
    assert_eq!(settings["morningStartMin"], 240);
}

/// 設定画面の選択肢を返すこと。選択肢がすべて更新の検証に通る (tzdb にある) ことは
/// `src/timezones.rs` のテストで確かめる。
#[sqlx::test]
async fn timezone_choices_are_listed(pool: SqlitePool) {
    let (app, cookie) = logged_in_app(pool).await;

    let response = send(&app, get("/api/v1/settings/timezones", Some(&cookie))).await;
    assert_eq!(response.status(), StatusCode::OK);

    let body = json_body(response).await;
    let zones = body["timezones"]
        .as_array()
        .expect("timezones should be an array");
    assert!(zones.len() > 100, "tzdb should be bundled");
    assert!(zones.iter().any(|zone| zone == "Asia/Tokyo"));
}

/// 自動設定のユーザーだけ、ブラウザのタイムゾーンで更新する。手動にしたユーザーは上書きしない。
#[sqlx::test]
async fn detected_timezone_is_applied_only_while_automatic(pool: SqlitePool) {
    let (app, cookie) = logged_in_app(pool).await;

    let response = send(
        &app,
        put_json(
            "/api/v1/settings/detected-timezone",
            Some(&cookie),
            json!({"timezone": "America/New_York"}),
        ),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let settings = json_body(response).await;
    assert_eq!(settings["timezone"], "America/New_York");
    assert_eq!(settings["timezoneAuto"], true);

    let response = send(
        &app,
        put_json(
            "/api/v1/settings",
            Some(&cookie),
            json!({
                "timezone": "Asia/Tokyo",
                "timezoneAuto": false,
                "morningStartMin": 240,
                "morningEndMin": 600,
                "eveningStartMin": 1080,
                "eveningEndMin": 1440
            }),
        ),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);

    let response = send(
        &app,
        put_json(
            "/api/v1/settings/detected-timezone",
            Some(&cookie),
            json!({"timezone": "Europe/London"}),
        ),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let settings = json_body(response).await;
    assert_eq!(settings["timezone"], "Asia/Tokyo");
    assert_eq!(settings["timezoneAuto"], false);
}
