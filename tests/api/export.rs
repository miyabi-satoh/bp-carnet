//! 記録のエクスポート (CSV)。

use super::*;

/// 書き出しのファイル名が `bp-records-YYYY-MM-DD.csv` (日付付き) であることを確かめる。
fn assert_export_filename(content_disposition: &str) {
    let name = content_disposition
        .split("filename=\"")
        .nth(1)
        .and_then(|rest| rest.strip_suffix('"'))
        .unwrap_or_else(|| panic!("filename が無い: {content_disposition}"));
    let date = name
        .strip_prefix("bp-records-")
        .and_then(|rest| rest.strip_suffix(".csv"))
        .unwrap_or_else(|| panic!("想定と違う名前: {name}"));
    assert!(
        date.len() == 10 && date.as_bytes()[4] == b'-' && date.as_bytes()[7] == b'-',
        "日付の形でない: {name}"
    );
}

/// CSV が `text/csv` の `Content-Disposition: attachment` で返り、主キーや登録日時を含まないことを
/// 確認する (docs/import-export.md)。CSV の書式 (BOM・ヘッダー・クォート) は `src/api/records.rs` の
/// `records_to_csv` のテスト。
#[sqlx::test]
async fn export_records_returns_csv_attachment(pool: SqlitePool) {
    let (app, cookie) = logged_in_app(pool).await;
    create_record(&app, &cookie, valid_record_body()).await;

    let response = send(&app, get("/api/v1/records/export", Some(&cookie))).await;

    assert_eq!(response.status(), StatusCode::OK);
    let header_value = |name: header::HeaderName| {
        response
            .headers()
            .get(&name)
            .unwrap_or_else(|| panic!("should set {name}"))
            .to_str()
            .expect("header value should be valid utf-8")
            .to_string()
    };
    assert!(header_value(header::CONTENT_TYPE).starts_with("text/csv"));
    let content_disposition = header_value(header::CONTENT_DISPOSITION);
    assert!(content_disposition.contains("attachment"));
    assert_export_filename(&content_disposition);
    let records = export_csv_rows(response).await;
    assert_eq!(records.len(), 1);
    assert_eq!(records[0]["memo"], "朝食前");
    assert!(
        !records[0].contains_key("id") && !records[0].contains_key("createdAt"),
        "export should not include id or createdAt"
    );
}

/// エクスポートは測定日時の古い順 (一覧表示の新しい順とは逆) で返すことを確認する
/// (Excel/再インポートで読むことを想定した時系列順)。
#[sqlx::test]
async fn export_records_are_ordered_chronologically(pool: SqlitePool) {
    let (app, cookie) = logged_in_app(pool).await;

    for measured_at in ["2026-09-10T08:00", "2026-09-01T08:00", "2026-09-05T08:00"] {
        create_record(
            &app,
            &cookie,
            json!({"localMeasuredAt": measured_at, "systolic": 120, "diastolic": 80}),
        )
        .await;
    }

    let response = send(&app, get("/api/v1/records/export", Some(&cookie))).await;
    let records = export_csv_rows(response).await;
    let measured_ats: Vec<_> = records.iter().map(|r| r["measuredAt"].as_str()).collect();
    assert_eq!(
        measured_ats,
        vec!["2026-09-01 08:00", "2026-09-05 08:00", "2026-09-10 08:00"]
    );
}

/// ユーザーごとにデータが完全分離されていること (他ユーザーの記録がエクスポートに
/// 含まれないこと) を確認する。
#[sqlx::test]
async fn export_records_only_returns_own_users_records(pool: SqlitePool) {
    insert_user(&pool, "alice", "password-a").await;
    insert_user(&pool, "bob", "password-b").await;
    let app = test_app(pool).await;

    let alice_cookie = login_and_get_cookie(&app, "alice", "password-a").await;
    let bob_cookie = login_and_get_cookie(&app, "bob", "password-b").await;
    create_record(&app, &alice_cookie, valid_record_body()).await;

    let response = send(&app, get("/api/v1/records/export", Some(&bob_cookie))).await;
    let records = export_csv_rows(response).await;
    assert_eq!(
        records.len(),
        0,
        "bob should not see alice's records in the export"
    );
}
