//! 血圧記録の登録・一覧取得・編集・削除。
//!
//! 日時はユーザーのタイムゾーンでの日時・日付で受け渡し、UTC との変換は `local_time` で行う。

use std::collections::BTreeSet;

use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use axum::http::header;
use axum::response::{IntoResponse, Response};
use jiff::civil::{Date, DateTime};
use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;

use crate::auth::AuthUser;
use crate::error::{AppError, AppJson, AppPath, AppQuery, ErrorResponse, RowError};
use crate::local_time::{self, UserTimeZone};
use crate::settings;
use crate::state::AppState;
use crate::stats;
use crate::validation::{
    BpValuesError, DIASTOLIC_RANGE, PULSE_RANGE, SYSTOLIC_RANGE, validate_bp_values,
};

#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CreateRecordRequest {
    /// 測定日時。ユーザーのタイムゾーンでの日時 (`YYYY-MM-DDTHH:MM`、オフセット無し)。
    #[schema(example = "2026-09-07T07:15")]
    local_measured_at: String,
    #[schema(example = 120)]
    systolic: i64,
    #[schema(example = 80)]
    diastolic: i64,
    #[schema(example = 65)]
    pulse: Option<i64>,
    #[serde(default)]
    memo: String,
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct RecordResponse {
    id: i64,
    /// ユーザーのタイムゾーンでの測定日時 (`YYYY-MM-DDTHH:MM`)。
    ///
    /// ADR: UTC の値は返さない。frontend が端末のタイムゾーンで表示してしまう余地を残さないため。
    #[schema(example = "2026-09-07T07:15")]
    local_measured_at: String,
    systolic: i64,
    diastolic: i64,
    pulse: Option<i64>,
    memo: String,
    /// 版番号。直す・消す・取り込むときに、この値をそのまま送る。
    #[schema(example = 1)]
    version: i64,
}

/// 測定時刻が朝/夜のどちらの時間帯か。
#[derive(Debug, Clone, Copy, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub enum DayPeriod {
    Morning,
    Evening,
}

impl From<stats::Slot> for DayPeriod {
    fn from(value: stats::Slot) -> Self {
        match value {
            stats::Slot::Morning => Self::Morning,
            stats::Slot::Evening => Self::Evening,
        }
    }
}

/// 一覧の1件。登録・更新の応答と違い、表示用に朝/夜の区分を付ける
/// (登録・更新の後は frontend が一覧を取り直すため、そちらには付けない)。
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct RecordListItem {
    #[serde(flatten)]
    record: RecordResponse,
    /// ユーザーのタイムゾーン・朝/夜の時間帯で判定した区分。どちらにも属さない時刻なら `null`。
    day_period: Option<DayPeriod>,
}

/// `bp_records` の1行。`measured_at` は保存形式の UTC。
struct RecordRow {
    id: i64,
    measured_at: String,
    systolic: i64,
    diastolic: i64,
    pulse: Option<i64>,
    memo: String,
    version: i64,
}

impl RecordResponse {
    /// 登録・更新・インポートの3経路で共通の組み立て。
    fn new(id: i64, version: i64, local: DateTime, payload: CreateRecordRequest) -> Self {
        Self {
            id,
            local_measured_at: local_time::format_local_datetime(local),
            systolic: payload.systolic,
            diastolic: payload.diastolic,
            pulse: payload.pulse,
            memo: payload.memo,
            version,
        }
    }

    fn from_row(row: RecordRow, local: DateTime) -> Self {
        Self {
            id: row.id,
            local_measured_at: local_time::format_local_datetime(local),
            systolic: row.systolic,
            diastolic: row.diastolic,
            pulse: row.pulse,
            memo: row.memo,
            version: row.version,
        }
    }
}

/// 保存形式の UTC を、ユーザーのタイムゾーンでの日時にする。書き込み時に正規化しているため、
/// 解釈できないのは DB を直接編集した場合だけ。
fn stored_local_datetime(tz: &UserTimeZone, measured_at: &str) -> Result<DateTime, AppError> {
    tz.local_datetime(measured_at)
        .ok_or_else(|| AppError::InvalidStoredDatetime(measured_at.to_string()))
}

/// リクエストの値域・整合性を検証し、測定日時を保存形式の UTC にする。
fn validate_and_normalize(
    payload: &CreateRecordRequest,
    tz: &UserTimeZone,
) -> Result<String, AppError> {
    validate_bp_values(payload.systolic, payload.diastolic, payload.pulse).map_err(|err| {
        AppError::Validation(match err {
            BpValuesError::SystolicOutOfRange => format!(
                "systolic must be within {}..={}",
                SYSTOLIC_RANGE.start(),
                SYSTOLIC_RANGE.end()
            ),
            BpValuesError::DiastolicOutOfRange => format!(
                "diastolic must be within {}..={}",
                DIASTOLIC_RANGE.start(),
                DIASTOLIC_RANGE.end()
            ),
            BpValuesError::SystolicNotGreaterThanDiastolic => {
                "systolic must be greater than diastolic".to_string()
            }
            BpValuesError::PulseOutOfRange => format!(
                "pulse must be within {}..={}",
                PULSE_RANGE.start(),
                PULSE_RANGE.end()
            ),
        })
    })?;

    let measured_at = tz
        .parse_local_datetime(&payload.local_measured_at)
        .ok_or_else(|| {
            AppError::Validation(
                "localMeasuredAt must be a local datetime in YYYY-MM-DDTHH:MM format".to_string(),
            )
        })?;
    // 明日以降の日付は拒む (年・月の打ち間違いで、普段の期間に出てこなくなるため)。
    // 日付で比べ、今日の先の時刻は通す (端末とサーバーの時計のずれで止めないため)。
    // 形式は上で確かめたので、先頭10文字は `YYYY-MM-DD` で、文字列の大小が日付の前後になる。
    if payload.local_measured_at[..10] > *tz.local_today() {
        return Err(AppError::Validation(
            "localMeasuredAt must not be after today".to_string(),
        ));
    }
    Ok(measured_at)
}

fn parse_date(value: &str, field_name: &str) -> Result<Date, AppError> {
    local_time::parse_local_date(value).ok_or_else(|| {
        AppError::Validation(format!("{field_name} must be a date in YYYY-MM-DD format"))
    })
}

/// `from > to` の場合にエラーにする。範囲指定 (一覧取得・集計・インポート) の
/// 全経路で使う共有ロジック。
fn ensure_from_not_after_to(from: Date, to: Date) -> Result<(), AppError> {
    if from > to {
        return Err(AppError::Validation(
            "from must not be after to".to_string(),
        ));
    }
    Ok(())
}

/// 期間の最初の日の始まり (保存形式の UTC、この値を含む)。
fn range_start(tz: &UserTimeZone, from: Date) -> Result<String, AppError> {
    tz.start_of_day_utc(from)
        .ok_or_else(|| AppError::Validation("from is out of range".to_string()))
}

/// 期間の最後の日の翌日の始まり (保存形式の UTC、この値は含まない)。日の終わりを 23:59:59 で
/// 表すと、秒未満や夏時間で1日の長さが変わる日に取りこぼすため、翌日の始まりで区切る。
fn range_end(tz: &UserTimeZone, to: Date) -> Result<String, AppError> {
    to.tomorrow()
        .ok()
        .and_then(|next| tz.start_of_day_utc(next))
        .ok_or_else(|| AppError::Validation("to is out of range".to_string()))
}

/// 期間の絞り込み (一覧取得・集計)。
#[derive(Debug, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
#[serde(rename_all = "camelCase")]
pub struct DateRangeQuery {
    /// 期間の最初の日 (ユーザーのタイムゾーンでの `YYYY-MM-DD`、この日を含む)。省略時は下限無し。
    #[param(format = Date, example = "2026-09-01")]
    from: Option<String>,
    /// 期間の最後の日 (ユーザーのタイムゾーンでの `YYYY-MM-DD`、この日を含む)。省略時は上限無し。
    #[param(format = Date, example = "2026-09-07")]
    to: Option<String>,
}

/// `from`/`to` を保存形式の UTC の `[開始, 終了)` にする。省略した側は `None`。
fn parse_date_range(
    tz: &UserTimeZone,
    query: &DateRangeQuery,
) -> Result<(Option<String>, Option<String>), AppError> {
    let from = query
        .from
        .as_deref()
        .map(|value| parse_date(value, "from"))
        .transpose()?;
    let to = query
        .to
        .as_deref()
        .map(|value| parse_date(value, "to"))
        .transpose()?;
    if let (Some(from), Some(to)) = (from, to) {
        ensure_from_not_after_to(from, to)?;
    }
    Ok((
        from.map(|from| range_start(tz, from)).transpose()?,
        to.map(|to| range_end(tz, to)).transpose()?,
    ))
}

/// `bp_records` へ1件INSERTし id を返す。登録・インポートの両方から呼ぶ
/// (`executor` はプール本体・トランザクションのどちらでも渡せるようにする)。
async fn insert_record<'e, E: sqlx::SqliteExecutor<'e>>(
    executor: E,
    user_id: i64,
    measured_at: &str,
    payload: &CreateRecordRequest,
) -> Result<i64, AppError> {
    let id = sqlx::query_scalar!(
        r#"
        INSERT INTO bp_records (user_id, measured_at, systolic, diastolic, pulse, memo)
        VALUES (?, ?, ?, ?, ?, ?)
        RETURNING id AS "id!: i64"
        "#,
        user_id,
        measured_at,
        payload.systolic,
        payload.diastolic,
        payload.pulse,
        payload.memo,
    )
    .fetch_one(executor)
    .await?;

    Ok(id)
}

#[utoipa::path(
    post,
    path = "/records",
    request_body = CreateRecordRequest,
    responses(
        (status = 201, description = "登録成功", body = RecordResponse),
        (status = 400, description = "リクエストボディが不正 (JSON として解釈できない等)", body = ErrorResponse),
        (status = 401, description = "未ログイン", body = ErrorResponse),
        (status = 415, description = "Content-Type が application/json でない", body = ErrorResponse),
        (status = 422, description = "必須フィールドが無い、または値域・日時形式が不正", body = ErrorResponse),
        (status = 500, description = "保存されているタイムゾーンが tzdb に存在しない", body = ErrorResponse),
    )
)]
async fn create_record(
    user: AuthUser,
    State(state): State<AppState>,
    AppJson(payload): AppJson<CreateRecordRequest>,
) -> Result<(StatusCode, Json<RecordResponse>), AppError> {
    let tz = settings::load_timezone(&state.pool, user.id).await?;
    let measured_at = validate_and_normalize(&payload, &tz)?;
    let id = insert_record(&state.pool, user.id, &measured_at, &payload).await?;
    let local = stored_local_datetime(&tz, &measured_at)?;
    Ok((
        StatusCode::CREATED,
        Json(RecordResponse::new(id, 1, local, payload)),
    ))
}

/// 一括置換1回あたりの上限件数。CSV インポート・OCR の複数行読み取りを想定し、朝夜2回
/// ×10年分 (7300件程度) を余裕を持ってカバーできる値にする。axum の body size limit
/// (デフォルト2MB) は、1件あたり実測200バイト程度として1万件でも2MBを超えないため
/// 問題にならない。
const MAX_IMPORT_RECORDS: usize = 10_000;

#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ImportRecordsRequest {
    /// `records` が収まっているべき範囲の最初の日 (ユーザーのタイムゾーンでの `YYYY-MM-DD`、
    /// この日を含む)。置き換えるのはこの範囲すべてではなく、`records` に行がある日だけ。
    #[schema(value_type = String, format = Date, example = "2026-09-01")]
    from: String,
    /// `records` が収まっているべき範囲の最後の日 (ユーザーのタイムゾーンでの `YYYY-MM-DD`、
    /// この日を含む)。
    #[schema(value_type = String, format = Date, example = "2026-09-07")]
    to: String,
    /// 取り込む行。行がある日だけを、その日ごと丸ごと置き換える。
    records: Vec<CreateRecordRequest>,
    /// 確認画面を出した時点の、置き換える日 (`records` に行がある日) の既存の記録。今のその日の記録と
    /// 過不足なく一致しなければ、ほかで変わったとして取り込まない (docs/import-export.md)。
    expected: Vec<RecordVersion>,
}

/// 記録の `id` と版番号の組。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct RecordVersion {
    id: i64,
    version: i64,
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ImportRecordsResponse {
    /// 置き換えた日 (`records` に行がある日) の中で削除された既存レコード数。
    deleted: u64,
    created: Vec<RecordResponse>,
}

#[utoipa::path(
    post,
    path = "/records/import",
    request_body = ImportRecordsRequest,
    responses(
        (status = 200, description = "置換成功 (`records` に行がある日だけを置き換える。\
            行が1件も無い日の記録には触らない)", body = ImportRecordsResponse),
        (status = 400, description = "リクエストボディが不正 (JSON として解釈できない等)", body = ErrorResponse),
        (status = 401, description = "未ログイン", body = ErrorResponse),
        (status = 415, description = "Content-Type が application/json でない", body = ErrorResponse),
        (status = 422, description = "records が空・上限件数超過、from/to が日付として解釈できない・\
            from が to より後、または1件以上の値域・日時形式が不正・from〜to の範囲外 \
            (`error.rows` に不正な行の index/message を列挙)", body = ErrorResponse),
        (status = 409, description = "置き換える日の記録が `expected` と一致しない (ほかで変わった)。\
            何も変えない", body = ErrorResponse),
        (status = 500, description = "保存されているタイムゾーンが tzdb に存在しない", body = ErrorResponse),
    )
)]
async fn import_records(
    user: AuthUser,
    State(state): State<AppState>,
    AppJson(payload): AppJson<ImportRecordsRequest>,
) -> Result<Json<ImportRecordsResponse>, AppError> {
    let from = parse_date(&payload.from, "from")?;
    let to = parse_date(&payload.to, "to")?;
    ensure_from_not_after_to(from, to)?;

    if payload.records.is_empty() {
        return Err(AppError::Validation(
            "records must not be empty".to_string(),
        ));
    }
    if payload.records.len() > MAX_IMPORT_RECORDS {
        return Err(AppError::Validation(format!(
            "records must not exceed {MAX_IMPORT_RECORDS} items"
        )));
    }

    let tz = settings::load_timezone(&state.pool, user.id).await?;
    let start = range_start(&tz, from)?;
    let end = range_end(&tz, to)?;

    // 1件でも不正な行があれば、どの行が問題かを (最初の1件で打ち切らず) すべて集めて返す。
    // CSVインポート・OCRの手書きメモ複数行読み取りのように、ユーザーが複数行をまとめて
    // 確認・修正する用途を想定しているため、1回の応答で全行の直し方が分かるようにする。
    // ここではさらに「from〜to に収まっているか」も検証する。置き換える日は行そのものから
    // 決まる (下記) ので冪等性には関わらないが、確認画面に出した範囲の外にある行を、
    // 利用者が見ないまま取り込んでしまうのを防ぐ歯止めとして残す。
    //
    // 併せて、保存形式からローカル日時に戻したものを持つ。置き換える日の判定と、応答に
    // 載せる日時を同じ値から作るため。
    let mut normalized = Vec::with_capacity(payload.records.len());
    let mut errors = Vec::new();
    for (index, record) in payload.records.iter().enumerate() {
        match validate_and_normalize(record, &tz) {
            Ok(measured_at) if measured_at < start || measured_at >= end => {
                errors.push(RowError {
                    index,
                    message: "localMeasuredAt must be within the from..=to dates".to_string(),
                });
            }
            Ok(measured_at) => {
                let local = stored_local_datetime(&tz, &measured_at)?;
                normalized.push((measured_at, local));
            }
            Err(err) => errors.push(RowError {
                index,
                message: err.to_string(),
            }),
        }
    }
    if !errors.is_empty() {
        return Err(AppError::BatchValidation {
            total: payload.records.len(),
            errors,
        });
    }

    // 置き換えるのは、取り込む行がある日だけ (docs/import-export.md)。行が1件も無い日の記録には
    // 触らない。from〜to をまるごと消すと、CSV や写真に書かれていない日の記録まで一括で
    // 消えてしまい、取り込みが一括削除の手段になってしまうため。
    // 日はユーザーのタイムゾーンで区切る。保存形式から戻した日時を使うことで、下の削除の範囲
    // (`range_start`/`range_end`) と同じ根拠にそろえる。
    let target_days: BTreeSet<Date> = normalized.iter().map(|(_, local)| local.date()).collect();

    // 検証が全行通ってから初めて書き込む: 削除・挿入を1トランザクションにまとめ、
    // 途中で失敗したら (通常は起こらない想定だが) 対象の日を空のまま残さない。
    //
    // ADR: 置き換える日の記録が `expected` と一致するかは、消す前に読んで確かめるのではなく、
    // 消した記録 (`RETURNING`) で確かめ、合わなければロールバックする。deferred なトランザクションで
    // 読みを先に置くと、後から書き込みへ昇格する際に他の接続のコミットと衝突して
    // `SQLITE_BUSY_SNAPSHOT` になりうる (`account::schedule_deletion` と同じ理由)。
    let mut tx = state.pool.begin().await?;
    let mut removed = BTreeSet::new();
    for day in &target_days {
        let day_start = range_start(&tz, *day)?;
        let day_end = range_end(&tz, *day)?;
        let versions = sqlx::query_as!(
            RecordVersion,
            r#"
            DELETE FROM bp_records
            WHERE user_id = ? AND measured_at >= ? AND measured_at < ?
            RETURNING id AS "id!: i64", version AS "version!: i64"
            "#,
            user.id,
            day_start,
            day_end,
        )
        .fetch_all(&mut *tx)
        .await?;
        removed.extend(versions);
    }
    // ここで返ると `tx` はコミットされずに捨てられ、消した記録は元に戻る。
    if removed != payload.expected.iter().copied().collect::<BTreeSet<_>>() {
        return Err(AppError::RecordConflict);
    }
    let deleted = removed.len() as u64;

    let mut created = Vec::with_capacity(payload.records.len());
    for (record, (measured_at, local)) in payload.records.into_iter().zip(normalized) {
        let id = insert_record(&mut *tx, user.id, &measured_at, &record).await?;
        created.push(RecordResponse::new(id, 1, local, record));
    }
    tx.commit().await?;

    Ok(Json(ImportRecordsResponse { deleted, created }))
}

#[utoipa::path(
    get,
    path = "/records",
    params(DateRangeQuery),
    responses(
        (status = 200, description = "測定日時の新しい順の一覧", body = Vec<RecordListItem>),
        (status = 401, description = "未ログイン", body = ErrorResponse),
        (status = 422, description = "from/to が日付として解釈できない、または from > to", body = ErrorResponse),
        (status = 500, description = "保存されているタイムゾーンが tzdb に存在しない", body = ErrorResponse),
    )
)]
async fn list_records(
    user: AuthUser,
    State(state): State<AppState>,
    AppQuery(query): AppQuery<DateRangeQuery>,
) -> Result<Json<Vec<RecordListItem>>, AppError> {
    let classifier = settings::load_slot_classifier(&state.pool, user.id).await?;
    Ok(Json(
        list_items(&state, user.id, &classifier, &query).await?,
    ))
}

/// 指定ユーザーの、期間内の記録を測定日時の新しい順に返す。本人の一覧と管理者の閲覧で共有する。
/// `classifier` はそのユーザーの個人設定から作ったもの ([`settings::load_slot_classifier`])。
pub async fn list_items(
    state: &AppState,
    user_id: i64,
    classifier: &stats::SlotClassifier,
    query: &DateRangeQuery,
) -> Result<Vec<RecordListItem>, AppError> {
    let (start, end) = parse_date_range(classifier.timezone(), query)?;

    let rows = sqlx::query_as!(
        RecordRow,
        r#"
        SELECT
            id AS "id!: i64",
            measured_at AS "measured_at!: String",
            systolic AS "systolic!: i64",
            diastolic AS "diastolic!: i64",
            pulse AS "pulse: i64",
            memo AS "memo!: String",
            version AS "version!: i64"
        FROM bp_records
        WHERE user_id = ?
          AND (? IS NULL OR measured_at >= ?)
          AND (? IS NULL OR measured_at < ?)
        ORDER BY measured_at DESC
        "#,
        user_id,
        start,
        start,
        end,
        end,
    )
    .fetch_all(&state.pool)
    .await?;

    rows.into_iter()
        .map(|row| {
            let local = stored_local_datetime(classifier.timezone(), &row.measured_at)?;
            Ok(RecordListItem {
                day_period: classifier.slot(local).map(Into::into),
                record: RecordResponse::from_row(row, local),
            })
        })
        .collect()
}

/// エクスポート1行分。主キーは含めない (インポートは日付で照合するため不要、docs/import-export.md)。
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ExportRecordResponse {
    /// ユーザーのタイムゾーンでの日時 (`YYYY-MM-DD HH:MM`)。表計算ソフトで開いたときに、画面・
    /// 印刷と同じ時刻に見え、日時として読まれるため (docs/import-export.md)。
    #[schema(example = "2026-09-25 07:15")]
    measured_at: String,
    systolic: i64,
    diastolic: i64,
    pulse: Option<i64>,
    memo: String,
}

/// CSV のヘッダー行・列順 (docs/import-export.md)。JSON のフィールド名 (`camelCase`) とそろえ、
/// エクスポートした CSV をそのまま読み替えられるようにする。
const CSV_HEADER: [&str; 5] = ["measuredAt", "systolic", "diastolic", "pulse", "memo"];

/// `ExportRecordResponse` の列を CSV (BOM 付き UTF-8・CRLF 改行) にシリアライズする。
/// BOM は Excel 等が UTF-8 と認識せず文字化けするのを防ぐため付与する。CRLF は RFC 4180
/// 準拠かつ Excel との相性がよい改行コード。
fn records_to_csv(rows: &[ExportRecordResponse]) -> Vec<u8> {
    let mut writer = csv::WriterBuilder::new()
        .terminator(csv::Terminator::CRLF)
        .from_writer(vec![0xEF, 0xBB, 0xBF]);
    writer
        .write_record(CSV_HEADER)
        .expect("writing to an in-memory buffer never fails");
    for row in rows {
        writer
            .write_record([
                row.measured_at.clone(),
                row.systolic.to_string(),
                row.diastolic.to_string(),
                row.pulse.map(|p| p.to_string()).unwrap_or_default(),
                row.memo.clone(),
            ])
            .expect("writing to an in-memory buffer never fails");
    }
    writer
        .into_inner()
        .expect("flushing an in-memory buffer never fails")
}

#[utoipa::path(
    get,
    path = "/records/export",
    responses(
        (
            status = 200,
            description = "全件のエクスポート結果 (測定日時の古い順)。BOM 付き UTF-8・CRLF 改行の CSV で、\
                先頭にヘッダー行を持つ。`Content-Disposition: attachment` を付与し、ファイル名は `bp-records-<ユーザーのタイムゾーンでの今日の日付>.csv`。\
                本文は CSV テキストのため `body` は String だが、列名と型は \
                `ExportRecordResponse` スキーマに載せてある (measuredAt, systolic, \
                diastolic, pulse, memo の順)",
            body = String,
            content_type = "text/csv",
        ),
        (status = 401, description = "未ログイン", body = ErrorResponse),
    )
)]
async fn export_records(
    user: AuthUser,
    State(state): State<AppState>,
) -> Result<Response, AppError> {
    let tz = settings::load_timezone(&state.pool, user.id).await?;
    // 設定画面には表示中の期間が無いため、常に全件を対象にする (docs/import-export.md)。
    let mut records = sqlx::query_as!(
        ExportRecordResponse,
        r#"
        SELECT
            measured_at AS "measured_at!: String",
            systolic AS "systolic!: i64",
            diastolic AS "diastolic!: i64",
            pulse AS "pulse: i64",
            memo AS "memo!: String"
        FROM bp_records
        WHERE user_id = ?
        ORDER BY measured_at ASC, id ASC
        "#,
        user.id,
    )
    .fetch_all(&state.pool)
    .await?;
    for record in &mut records {
        // 保存形式の UTC を、ユーザーのタイムゾーンの時刻にする。
        record.measured_at =
            local_time::format_csv_datetime(stored_local_datetime(&tz, &record.measured_at)?);
    }

    // 書き出すたびに同じ名前だと、保存先で前のファイルと見分けられないため、今日の日付を付ける。
    // フロントはこの名前で保存する (frontend/src/lib/export.ts)。
    let content_disposition = format!(
        "attachment; filename=\"bp-records-{}.csv\"",
        tz.local_today()
    );
    Ok((
        [
            (header::CONTENT_TYPE, "text/csv; charset=utf-8".to_string()),
            (header::CONTENT_DISPOSITION, content_disposition),
        ],
        records_to_csv(&records),
    )
        .into_response())
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UpdateRecordRequest {
    #[serde(flatten)]
    record: CreateRecordRequest,
    /// フォームを開いた時点の版番号。今の記録と違えば更新しない。
    #[schema(example = 1)]
    version: i64,
}

/// 版番号が合わずに更新・削除できなかったとき、記録がまだあるか (ほかで変わった) か、
/// もう無い (ほかで消された・他ユーザーのもの) かで返すエラーを分ける。
async fn conflict_or_not_found(state: &AppState, user_id: i64, id: i64) -> AppError {
    let exists = sqlx::query_scalar!(
        r#"SELECT EXISTS (SELECT 1 FROM bp_records WHERE id = ? AND user_id = ?) AS "exists!: bool""#,
        id,
        user_id,
    )
    .fetch_one(&state.pool)
    .await;
    match exists {
        Ok(true) => AppError::RecordConflict,
        Ok(false) => AppError::NotFound,
        Err(err) => err.into(),
    }
}

#[utoipa::path(
    get,
    path = "/records/{id}",
    params(("id" = i64, Path, description = "レコードID")),
    responses(
        (status = 200, description = "記録1件", body = RecordResponse),
        (status = 400, description = "id がパスパラメータとして解釈できない (数値でない等)", body = ErrorResponse),
        (status = 401, description = "未ログイン", body = ErrorResponse),
        (status = 404, description = "指定した id のレコードが存在しない、または他ユーザーのもの", body = ErrorResponse),
        (status = 500, description = "保存されているタイムゾーンが tzdb に存在しない", body = ErrorResponse),
    )
)]
async fn get_record(
    user: AuthUser,
    State(state): State<AppState>,
    AppPath(id): AppPath<i64>,
) -> Result<Json<RecordResponse>, AppError> {
    let tz = settings::load_timezone(&state.pool, user.id).await?;
    let row = sqlx::query_as!(
        RecordRow,
        r#"
        SELECT
            id AS "id!: i64",
            measured_at AS "measured_at!: String",
            systolic AS "systolic!: i64",
            diastolic AS "diastolic!: i64",
            pulse AS "pulse: i64",
            memo AS "memo!: String",
            version AS "version!: i64"
        FROM bp_records
        WHERE id = ? AND user_id = ?
        "#,
        id,
        user.id,
    )
    .fetch_optional(&state.pool)
    .await?
    .ok_or(AppError::NotFound)?;
    let local = stored_local_datetime(&tz, &row.measured_at)?;
    Ok(Json(RecordResponse::from_row(row, local)))
}

#[utoipa::path(
    put,
    path = "/records/{id}",
    params(("id" = i64, Path, description = "更新対象のレコードID")),
    request_body = UpdateRecordRequest,
    responses(
        (status = 200, description = "更新成功", body = RecordResponse),
        (status = 400, description = "リクエストボディが不正 (JSON として解釈できない等)、または id がパスパラメータとして解釈できない (数値でない等)", body = ErrorResponse),
        (status = 401, description = "未ログイン", body = ErrorResponse),
        (status = 404, description = "指定した id のレコードが存在しない、または他ユーザーのもの", body = ErrorResponse),
        (status = 409, description = "version が今の記録と違う (ほかで変わった)。更新しない", body = ErrorResponse),
        (status = 415, description = "Content-Type が application/json でない", body = ErrorResponse),
        (status = 422, description = "必須フィールドが無い、または値域・日時形式が不正", body = ErrorResponse),
        (status = 500, description = "保存されているタイムゾーンが tzdb に存在しない", body = ErrorResponse),
    )
)]
async fn update_record(
    user: AuthUser,
    State(state): State<AppState>,
    AppPath(id): AppPath<i64>,
    AppJson(payload): AppJson<UpdateRecordRequest>,
) -> Result<Json<RecordResponse>, AppError> {
    let tz = settings::load_timezone(&state.pool, user.id).await?;
    let UpdateRecordRequest { record, version } = payload;
    let measured_at = validate_and_normalize(&record, &tz)?;

    // 他ユーザーの id を指定された場合も含め、今の記録が無ければ「存在しない」として 404 にする
    // (所有者以外に「存在するが自分のものではない」ことを教えない)。
    let result = sqlx::query!(
        r#"
        UPDATE bp_records
        SET measured_at = ?, systolic = ?, diastolic = ?, pulse = ?, memo = ?, version = version + 1
        WHERE id = ? AND user_id = ? AND version = ?
        "#,
        measured_at,
        record.systolic,
        record.diastolic,
        record.pulse,
        record.memo,
        id,
        user.id,
        version,
    )
    .execute(&state.pool)
    .await?;

    if result.rows_affected() == 0 {
        return Err(conflict_or_not_found(&state, user.id, id).await);
    }

    let local = stored_local_datetime(&tz, &measured_at)?;
    Ok(Json(RecordResponse::new(id, version + 1, local, record)))
}

/// 削除の条件。
#[derive(Debug, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
#[serde(rename_all = "camelCase")]
pub struct DeleteRecordQuery {
    /// 削除を決めた時点の版番号。今の記録と違えば削除しない。
    version: i64,
}

#[utoipa::path(
    delete,
    path = "/records/{id}",
    params(("id" = i64, Path, description = "削除対象のレコードID"), DeleteRecordQuery),
    responses(
        (status = 204, description = "削除成功"),
        (status = 400, description = "id がパスパラメータとして解釈できない (数値でない等)、または version が無い・数値でない", body = ErrorResponse),
        (status = 401, description = "未ログイン", body = ErrorResponse),
        (status = 404, description = "指定した id のレコードが存在しない、または他ユーザーのもの", body = ErrorResponse),
        (status = 409, description = "version が今の記録と違う (ほかで変わった)。削除しない", body = ErrorResponse),
    )
)]
async fn delete_record(
    user: AuthUser,
    State(state): State<AppState>,
    AppPath(id): AppPath<i64>,
    AppQuery(query): AppQuery<DeleteRecordQuery>,
) -> Result<StatusCode, AppError> {
    let result = sqlx::query!(
        "DELETE FROM bp_records WHERE id = ? AND user_id = ? AND version = ?",
        id,
        user.id,
        query.version,
    )
    .execute(&state.pool)
    .await?;

    if result.rows_affected() == 0 {
        return Err(conflict_or_not_found(&state, user.id, id).await);
    }

    Ok(StatusCode::NO_CONTENT)
}

/// 上下の血圧の平均。対象の記録が無い区分は `null` になる。
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct BpAverageResponse {
    #[schema(example = 128.5)]
    systolic: f64,
    #[schema(example = 82.0)]
    diastolic: f64,
}

impl From<stats::BpAverage> for BpAverageResponse {
    fn from(value: stats::BpAverage) -> Self {
        Self {
            systolic: value.systolic,
            diastolic: value.diastolic,
        }
    }
}

/// グラフ用の1日分。
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DailyAveragesResponse {
    /// ユーザーのタイムゾーンでの日付 (`YYYY-MM-DD`)。
    #[schema(example = "2026-09-08")]
    date: String,
    morning: Option<BpAverageResponse>,
    evening: Option<BpAverageResponse>,
}

/// 期間全体の朝/夜の平均と、グラフ用の日別系列。
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct RecordsSummaryResponse {
    morning: Option<BpAverageResponse>,
    evening: Option<BpAverageResponse>,
    /// 日付の昇順。朝夜どちらの時間帯にも記録が無い日は含まれない。
    days: Vec<DailyAveragesResponse>,
}

#[utoipa::path(
    get,
    path = "/records/summary",
    params(DateRangeQuery),
    responses(
        (status = 200, description = "集計成功", body = RecordsSummaryResponse),
        (status = 401, description = "未ログイン", body = ErrorResponse),
        (status = 422, description = "from/to が日付として解釈できない、または from > to", body = ErrorResponse),
        (status = 500, description = "保存されているタイムゾーンが tzdb に存在しない", body = ErrorResponse),
    )
)]
async fn summarize_records(
    user: AuthUser,
    State(state): State<AppState>,
    AppQuery(query): AppQuery<DateRangeQuery>,
) -> Result<Json<RecordsSummaryResponse>, AppError> {
    let classifier = settings::load_slot_classifier(&state.pool, user.id).await?;
    Ok(Json(summarize(&state, user.id, &classifier, &query).await?))
}

/// 指定ユーザーの、期間内の朝/夜の平均と日別系列を返す。本人の集計と管理者の閲覧で共有する。
/// `classifier` はそのユーザーの個人設定から作ったもの ([`settings::load_slot_classifier`])。
pub async fn summarize(
    state: &AppState,
    user_id: i64,
    classifier: &stats::SlotClassifier,
    query: &DateRangeQuery,
) -> Result<RecordsSummaryResponse, AppError> {
    let (start, end) = parse_date_range(classifier.timezone(), query)?;

    let rows = sqlx::query!(
        r#"
        SELECT
            measured_at AS "measured_at!: String",
            systolic AS "systolic!: i64",
            diastolic AS "diastolic!: i64"
        FROM bp_records
        WHERE user_id = ?
          AND (? IS NULL OR measured_at >= ?)
          AND (? IS NULL OR measured_at < ?)
        "#,
        user_id,
        start,
        start,
        end,
        end,
    )
    .fetch_all(&state.pool)
    .await?;

    let records: Vec<stats::RecordPoint> = rows
        .into_iter()
        .map(|row| stats::RecordPoint {
            measured_at: row.measured_at,
            systolic: row.systolic,
            diastolic: row.diastolic,
        })
        .collect();

    let summary = stats::summarize(&records, classifier);

    Ok(RecordsSummaryResponse {
        morning: summary.morning.map(Into::into),
        evening: summary.evening.map(Into::into),
        days: summary
            .days
            .into_iter()
            .map(|day| DailyAveragesResponse {
                date: day.date,
                morning: day.morning.map(Into::into),
                evening: day.evening.map(Into::into),
            })
            .collect(),
    })
}

/// 最新の記録の日。
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct LatestRecordDateResponse {
    /// 測定日時が最も新しい記録の、ユーザーのタイムゾーンでの日付 (`YYYY-MM-DD`)。記録が1件も無ければ `null`。
    /// 未来の日時の記録 (誤入力など) は数えない。
    #[schema(example = "2026-09-08")]
    date: Option<String>,
}

#[utoipa::path(
    get,
    path = "/records/latest-date",
    responses(
        (status = 200, description = "最新の記録の日 (記録が無ければ null)", body = LatestRecordDateResponse),
        (status = 401, description = "未ログイン", body = ErrorResponse),
        (status = 500, description = "保存されているタイムゾーンが tzdb に存在しない", body = ErrorResponse),
    )
)]
async fn latest_record_date(
    user: AuthUser,
    State(state): State<AppState>,
) -> Result<Json<LatestRecordDateResponse>, AppError> {
    let tz = settings::load_timezone(&state.pool, user.id).await?;
    let latest = sqlx::query_scalar!(
        r#"SELECT MAX(measured_at) AS "latest: String" FROM bp_records
           WHERE user_id = ? AND measured_at <= strftime('%Y-%m-%dT%H:%M:%SZ', 'now')"#,
        user.id,
    )
    .fetch_one(&state.pool)
    .await?;

    // 保存された日時が読めないときは `null` (frontend は今週を出す)。最新の日が分からないだけで画面は
    // 使えるため、一覧のようにエラーにはしない。壊れた行そのものは一覧の側で表に出る。
    let date = latest
        .and_then(|measured_at| tz.local_datetime(&measured_at))
        .map(|local| local.date().to_string());
    Ok(Json(LatestRecordDateResponse { date }))
}

pub fn router() -> OpenApiRouter<AppState> {
    // create_record (POST) と list_records (GET) は同一パス "/records" なので、
    // auth::router() と同様に1回の routes!() にまとめる。update_record (PUT) と
    // get_record・update_record・delete_record はパスが "/records/{id}" と異なるため別の .routes() に分ける。
    // import_records / export_records も "/records/import" · "/records/export" という
    // 別パスなので同様に分ける (axum は静的セグメント "import"/"export"/"summary" を
    // "{id}" より優先してマッチするため、"/records/{id}" 側とは衝突しない)。
    OpenApiRouter::new()
        .routes(routes!(create_record, list_records))
        .routes(routes!(import_records))
        .routes(routes!(export_records))
        .routes(routes!(summarize_records))
        .routes(routes!(latest_record_date))
        .routes(routes!(get_record, update_record, delete_record))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tokyo() -> UserTimeZone {
        UserTimeZone::get("Asia/Tokyo").expect("既知のタイムゾーンのはず")
    }

    fn record(
        local_measured_at: &str,
        systolic: i64,
        diastolic: i64,
        pulse: Option<i64>,
    ) -> CreateRecordRequest {
        CreateRecordRequest {
            local_measured_at: local_measured_at.to_string(),
            systolic,
            diastolic,
            pulse,
            memo: String::new(),
        }
    }

    // 値域の境界は `validation` のテストで確かめる。ここでは、`BpValuesError` のどの種類も
    // 検証のエラーになることを、境界から離れた値で1件ずつ確かめる。
    #[test]
    fn validate_and_normalize_rejects_each_invalid_value() {
        let tz = tokyo();
        for (name, payload) in [
            (
                "systolic out of range",
                record("2026-09-07T08:30", 300, 80, None),
            ),
            (
                "diastolic out of range",
                record("2026-09-07T08:30", 120, 10, None),
            ),
            (
                "systolic not above diastolic",
                record("2026-09-07T08:30", 90, 100, None),
            ),
            (
                "pulse out of range",
                record("2026-09-07T08:30", 120, 80, Some(300)),
            ),
            ("not a datetime", record("not-a-datetime", 120, 80, None)),
            (
                "datetime with an offset",
                record("2026-09-07T08:30+09:00", 120, 80, None),
            ),
        ] {
            assert!(
                matches!(
                    validate_and_normalize(&payload, &tz),
                    Err(AppError::Validation(_))
                ),
                "{name} should be rejected"
            );
        }
    }

    #[test]
    fn validate_and_normalize_converts_the_local_datetime_to_utc() {
        assert_eq!(
            validate_and_normalize(&record("2026-09-07T07:15", 120, 80, Some(65)), &tokyo())
                .expect("有効な記録のはず"),
            "2026-09-06T22:15:00Z"
        );
    }

    #[test]
    fn validate_and_normalize_rejects_a_date_after_today_but_accepts_later_today() {
        let tz = tokyo();
        let today = local_time::parse_local_date(&tz.local_today()).expect("日付のはず");
        let tomorrow = today.tomorrow().expect("翌日があるはず");

        assert!(matches!(
            validate_and_normalize(&record(&format!("{tomorrow}T00:00"), 120, 80, None), &tz),
            Err(AppError::Validation(_))
        ));
        validate_and_normalize(&record(&format!("{today}T23:59"), 120, 80, None), &tz)
            .expect("今日なら先の時刻でも通るはず");
    }

    fn export_row(measured_at: &str, pulse: Option<i64>, memo: &str) -> ExportRecordResponse {
        ExportRecordResponse {
            measured_at: measured_at.to_string(),
            systolic: 120,
            diastolic: 80,
            pulse,
            memo: memo.to_string(),
        }
    }

    #[test]
    fn records_to_csv_has_a_bom_and_a_header_even_without_rows() {
        assert_eq!(
            records_to_csv(&[]),
            b"\xEF\xBB\xBFmeasuredAt,systolic,diastolic,pulse,memo\r\n"
        );
    }

    // 値にカンマ・ダブルクォート・改行が入っていても、行が崩れないようにクォートする。
    #[test]
    fn records_to_csv_quotes_values_with_commas_quotes_and_newlines() {
        let csv = records_to_csv(&[
            export_row("2026-09-07 08:30", None, "朝,晩\"服薬\"\n異常なし"),
            export_row("2026-09-08 08:30", Some(65), ""),
        ]);
        let text = String::from_utf8(csv[3..].to_vec()).expect("UTF-8 のはず");
        assert_eq!(
            text,
            "measuredAt,systolic,diastolic,pulse,memo\r\n\
             2026-09-07 08:30,120,80,,\"朝,晩\"\"服薬\"\"\n異常なし\"\r\n\
             2026-09-08 08:30,120,80,65,\r\n"
        );
    }

    fn range(from: Option<&str>, to: Option<&str>) -> DateRangeQuery {
        DateRangeQuery {
            from: from.map(str::to_string),
            to: to.map(str::to_string),
        }
    }

    #[test]
    fn parse_date_range_rejects_a_non_date_and_from_after_to() {
        let tz = tokyo();
        for (name, query) in [
            (
                "from with a time",
                range(Some("2026-09-01T00:00:00Z"), None),
            ),
            ("to that is not a date", range(None, Some("2026-02-30"))),
            (
                "from after to",
                range(Some("2026-09-10"), Some("2026-09-01")),
            ),
        ] {
            assert!(
                matches!(parse_date_range(&tz, &query), Err(AppError::Validation(_))),
                "{name} should be rejected"
            );
        }
    }

    // `to` の日を丸1日含めるため、終わりは翌日の始まり (含まない) にする。
    #[test]
    fn parse_date_range_covers_whole_days_in_the_user_timezone() {
        assert_eq!(
            parse_date_range(&tokyo(), &range(Some("2026-09-05"), Some("2026-09-05")))
                .expect("有効な期間のはず"),
            (
                Some("2026-09-04T15:00:00Z".to_string()),
                Some("2026-09-05T15:00:00Z".to_string())
            )
        );
        assert_eq!(
            parse_date_range(&tokyo(), &range(None, None)).expect("省略は有効のはず"),
            (None, None)
        );
    }
}
