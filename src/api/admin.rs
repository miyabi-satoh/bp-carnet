//! 管理者によるユーザー管理 (`/api/v1/admin/*`、docs/authentication.md)。

use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use serde::{Deserialize, Serialize};
use sqlx::SqliteConnection;
use utoipa::ToSchema;
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;

use crate::account::{self, AdminDeletionCancel, DeletionOrigin};
use crate::api::account::DeletionResponse;
use crate::api::records::{self, DateRangeQuery, RecordListItem, RecordsSummaryResponse};
use crate::audit;
use crate::auth::{self, AdminUser, NewPassword, Role};
use crate::error::{AppError, AppJson, AppPath, AppQuery, ErrorResponse};
use crate::password_notice;
use crate::payments::ocr_quota;
use crate::settings;
use crate::state::AppState;
use crate::validation;

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct AdminUserResponse {
    id: i64,
    username: String,
    /// 表示名 (Google ログインで取得したもの)。一覧では、あればこちらを見出しに使う。
    display_name: Option<String>,
    role: Role,
    frozen: bool,
    /// このユーザー個別の OCR 累計金額の上限 (円)。`null` は設定の既定値 (`ocrDefaultBudgetYen`) を使う。
    /// 負値は無制限。
    ocr_budget_yen: Option<i64>,
    /// これまでの読み取りに使った額 (円、小数は切り上げ)。
    ocr_spent_yen: i64,
    /// 買い足した枠の残り (円、小数は切り上げ)。無ければ 0。
    ocr_paid_remaining_yen: i64,
    /// ID/PW ログインに使えるパスワードを持つか。`false` (Google 専用アカウント) には
    /// 管理者からパスワードを再設定できないため、一覧のボタンを出さない。
    password_usable: bool,
    /// 削除予定日時 (UTC、RFC3339)。`null` なら予約なし。
    #[schema(example = "2026-10-12T00:00:00.000Z")]
    deletion_scheduled_at: Option<String>,
    /// 削除予約の起点。`null` なら予約なし。管理者が取り消せるのは `admin` だけ (`inactive` は自動退会) なので、
    /// 一覧の取り消しボタンの出し分けに使う。
    deletion_origin: Option<DeletionOrigin>,
    /// 最終アクセス (UTC、RFC3339)。ログインとセッションを使った操作で、1日に1回まで更新される。
    #[schema(example = "2026-09-20T00:00:00.000Z")]
    last_seen_at: String,
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct AdminUsersResponse {
    users: Vec<AdminUserResponse>,
    /// `ocrBudgetYen` が `null` のユーザーに適用される既定値 (環境変数 `OCR_FREE_BUDGET_YEN`)。負値は無制限。
    ocr_default_budget_yen: i64,
}

/// ユーザーの一覧を返す。
///
/// ADR: 一覧の取得は `admin_audit_log` に残さない。記録は「誰に対して」を持つ操作が対象で
/// (`target_user_id` は NOT NULL)、対象の定まらない一覧取得とは噛み合わないため。個別の
/// アカウント情報・血圧記録の閲覧から記録する。
#[utoipa::path(
    get,
    path = "/admin/users",
    responses(
        (status = 200, description = "ユーザーの一覧", body = AdminUsersResponse),
        (status = 401, description = "未ログイン", body = ErrorResponse),
        (status = 403, description = "管理者ではない", body = ErrorResponse),
    )
)]
async fn list_users(
    _admin: AdminUser,
    State(state): State<AppState>,
) -> Result<Json<AdminUsersResponse>, AppError> {
    // 並び順は作成順 (id 昇順)。管理者が自分で作ったユーザーが、作った順に並ぶのが追いやすい。
    // 未確認のアカウント (サインアップの確認待ち) は、記録も操作の対象になる状態も持たないため出さない。
    // 一覧と買い足し枠を同じ時点で読む。
    let mut tx = state.pool.begin().await?;
    let rows = sqlx::query!(
        r#"SELECT id AS "id!: i64", username, display_name, role AS "role: Role",
                  frozen, ocr_budget_yen, ocr_spent_milli_yen,
                  password_usable, deletion_scheduled_at,
                  deletion_origin AS "deletion_origin: DeletionOrigin",
                  COALESCE(last_seen_at, created_at) AS "last_seen_at!: String"
           FROM users WHERE email_verified = 1 ORDER BY id"#
    )
    .fetch_all(&mut *tx)
    .await?;
    let mut paid = ocr_quota::summaries(&mut tx).await?;
    tx.commit().await?;

    Ok(Json(AdminUsersResponse {
        users: rows
            .into_iter()
            .map(|row| {
                let paid = paid.remove(&row.id);
                AdminUserResponse {
                    id: row.id,
                    username: row.username,
                    display_name: row.display_name,
                    role: row.role,
                    frozen: row.frozen != 0,
                    ocr_budget_yen: row.ocr_budget_yen,
                    ocr_spent_yen: (row.ocr_spent_milli_yen + 999) / 1000,
                    ocr_paid_remaining_yen: paid.map_or(0, |paid| (paid + 999) / 1000),
                    password_usable: row.password_usable != 0,
                    deletion_scheduled_at: row.deletion_scheduled_at,
                    deletion_origin: row.deletion_origin,
                    last_seen_at: row.last_seen_at,
                }
            })
            .collect(),
        ocr_default_budget_yen: state.ocr_free_budget_yen,
    }))
}

/// 管理者が閲覧するアカウント情報。朝/夜の平均は本人の時間帯で分けているため、読み方に要る
/// タイムゾーンと時間帯も含める。
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct AdminUserDetailResponse {
    id: i64,
    username: String,
    display_name: Option<String>,
    role: Role,
    frozen: bool,
    /// 削除予定日時 (UTC、RFC3339)。`null` なら予約なし。
    deletion_scheduled_at: Option<String>,
    /// 登録日時 (UTC、RFC3339)。
    #[schema(example = "2026-09-07T00:00:00.000Z")]
    created_at: String,
    /// ID/PW ログインに使えるパスワードを持つか。
    password_usable: bool,
    /// Google アカウントと紐付いているか。
    google_linked: bool,
    /// LINE アカウントと紐付いているか。
    line_linked: bool,
    /// Apple ID と紐付いているか。
    apple_linked: bool,
    /// IANA タイムゾーン名。
    #[schema(example = "Asia/Tokyo")]
    timezone: String,
    morning_start_min: i64,
    morning_end_min: i64,
    evening_start_min: i64,
    evening_end_min: i64,
}

/// ユーザーのアカウント情報を返す (docs/authentication.md)。`admin_audit_log` に `view_account` を記録する。
#[utoipa::path(
    get,
    path = "/admin/users/{id}",
    params(("id" = i64, Path, description = "対象ユーザーの id")),
    responses(
        (status = 200, description = "アカウント情報", body = AdminUserDetailResponse),
        (status = 400, description = "id がパスパラメータとして解釈できない (数値でない等)", body = ErrorResponse),
        (status = 401, description = "未ログイン", body = ErrorResponse),
        (status = 403, description = "管理者ではない", body = ErrorResponse),
        (status = 404, description = "対象のユーザーが存在しない", body = ErrorResponse),
    )
)]
async fn get_user(
    AdminUser(admin): AdminUser,
    State(state): State<AppState>,
    AppPath(target_id): AppPath<i64>,
) -> Result<Json<AdminUserDetailResponse>, AppError> {
    let row = sqlx::query!(
        r#"SELECT id AS "id!: i64", username, display_name, role AS "role: Role", frozen,
                  deletion_scheduled_at, created_at, password_usable,
                  EXISTS (SELECT 1 FROM oauth_identities
                          WHERE user_id = users.id AND provider = 'google') AS "google_linked!: bool",
                  EXISTS (SELECT 1 FROM oauth_identities
                          WHERE user_id = users.id AND provider = 'line') AS "line_linked!: bool",
                  EXISTS (SELECT 1 FROM oauth_identities
                          WHERE user_id = users.id AND provider = 'apple') AS "apple_linked!: bool",
                  timezone, morning_start_min, morning_end_min, evening_start_min, evening_end_min
           FROM users WHERE id = ? AND email_verified = 1"#,
        target_id
    )
    .fetch_optional(&state.pool)
    .await?
    .ok_or(AppError::NotFound)?;

    // 記録できなければ返さない。閲覧だけが成立して記録が残らない状態を作らないため。
    audit::record(&state.pool, admin.id, target_id, audit::Action::ViewAccount).await?;

    Ok(Json(AdminUserDetailResponse {
        id: row.id,
        username: row.username,
        display_name: row.display_name,
        role: row.role,
        frozen: row.frozen != 0,
        deletion_scheduled_at: row.deletion_scheduled_at,
        created_at: row.created_at,
        password_usable: row.password_usable != 0,
        google_linked: row.google_linked,
        line_linked: row.line_linked,
        apple_linked: row.apple_linked,
        timezone: row.timezone,
        morning_start_min: row.morning_start_min,
        morning_end_min: row.morning_end_min,
        evening_start_min: row.evening_start_min,
        evening_end_min: row.evening_end_min,
    }))
}

/// 管理者が閲覧する血圧記録。本人の画面が一覧と集計を別々に取るのと違い、1回の取得にまとめる。
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct AdminUserRecordsResponse {
    /// 測定日時の新しい順。
    records: Vec<RecordListItem>,
    summary: RecordsSummaryResponse,
}

/// ユーザーの血圧記録を返す (docs/authentication.md)。日付・朝/夜の区分は対象ユーザーの個人設定で扱う。
///
/// ADR: 一覧と集計を1つのエンドポイントにまとめる。取得のたびに `view_records` を記録するため、
/// 別々にすると期間を1回移すごとに記録が2件並ぶ。
#[utoipa::path(
    get,
    path = "/admin/users/{id}/records",
    params(("id" = i64, Path, description = "対象ユーザーの id"), DateRangeQuery),
    responses(
        (status = 200, description = "期間内の記録と集計", body = AdminUserRecordsResponse),
        (status = 400, description = "id がパスパラメータとして解釈できない (数値でない等)", body = ErrorResponse),
        (status = 401, description = "未ログイン", body = ErrorResponse),
        (status = 403, description = "管理者ではない", body = ErrorResponse),
        (status = 404, description = "対象のユーザーが存在しない", body = ErrorResponse),
        (status = 422, description = "from/to が日付として解釈できない、または from > to", body = ErrorResponse),
        (status = 500, description = "保存されているタイムゾーンが tzdb に存在しない", body = ErrorResponse),
    )
)]
async fn list_user_records(
    AdminUser(admin): AdminUser,
    State(state): State<AppState>,
    AppPath(target_id): AppPath<i64>,
    AppQuery(query): AppQuery<DateRangeQuery>,
) -> Result<Json<AdminUserRecordsResponse>, AppError> {
    if !auth::verified_user_exists(&state.pool, target_id).await? {
        return Err(AppError::NotFound);
    }

    let classifier = settings::load_slot_classifier(&state.pool, target_id).await?;
    let items = records::list_items(&state, target_id, &classifier, &query).await?;
    let summary = records::summarize(&state, target_id, &classifier, &query).await?;

    audit::record(&state.pool, admin.id, target_id, audit::Action::ViewRecords).await?;

    Ok(Json(AdminUserRecordsResponse {
        records: items,
        summary,
    }))
}

/// 対象ユーザーの権限。
///
/// ADR: 更新を済ませた後に読むこと。deferred なトランザクションで読みを先に置くと、後から
/// 書き込みへ昇格する際に他の接続のコミットと衝突して `SQLITE_BUSY_SNAPSHOT` になりうる
/// (`account::schedule_deletion` と同じ理由)。
async fn target_role(conn: &mut SqliteConnection, target_id: i64) -> Result<Role, AppError> {
    Ok(sqlx::query_scalar!(
        r#"SELECT role AS "role: Role" FROM users WHERE id = ?"#,
        target_id
    )
    .fetch_one(&mut *conn)
    .await?)
}

/// 有効な管理者が1人も残らないなら `error` を返す。呼び出し側のトランザクションごと
/// ロールバックされるので、直前の更新はなかったことになる。
///
/// 削除を予約済みの管理者は数えない。猶予期間を過ぎれば物理削除され、残るのがその1人
/// だけだと誰も管理できなくなる。
///
/// 管理者を対象にした操作でだけ呼ぶこと。一般ユーザーへの操作で管理者の数は変わらないため、
/// 無条件に確かめると「もともと0人」の状態で無関係な操作まで拒んでしまう。
async fn ensure_an_admin_remains(
    conn: &mut SqliteConnection,
    error: AppError,
) -> Result<(), AppError> {
    let remaining = sqlx::query_scalar!(
        r#"SELECT COUNT(*) AS "count!: i64" FROM users
           WHERE role = 'admin' AND frozen = 0 AND deletion_scheduled_at IS NULL"#
    )
    .fetch_one(&mut *conn)
    .await?;
    if remaining == 0 {
        return Err(error);
    }
    Ok(())
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SetFrozenRequest {
    frozen: bool,
}

/// ユーザーを凍結する / 凍結を解除する (docs/authentication.md)。凍結中はログイン・セッション継続の
/// どちらも弾かれる。
///
/// 凍結できないのは次の2つ。どちらも「誰も管理できない状態」を作るため。
///
/// - 自分自身 (凍結した時点で自分が締め出される)
/// - 有効な管理者が残らなくなる操作。自分自身を弾いている以上、単独のリクエストでは起きないが、
///   2人の管理者が互いを同時に凍結すると両方が通りうる。判定と更新を同じトランザクションに
///   入れて防ぐ
#[utoipa::path(
    put,
    path = "/admin/users/{id}/frozen",
    params(("id" = i64, Path, description = "対象ユーザーの id")),
    request_body = SetFrozenRequest,
    responses(
        (status = 204, description = "変更した"),
        (status = 400, description = "リクエストボディが不正 (JSON として解釈できない等)、または id がパスパラメータとして解釈できない (数値でない等)", body = ErrorResponse),
        (status = 401, description = "未ログイン", body = ErrorResponse),
        (status = 403, description = "管理者ではない", body = ErrorResponse),
        (status = 404, description = "対象のユーザーが存在しない", body = ErrorResponse),
        (status = 409, description = "自分自身、または有効な管理者が残らなくなる凍結", body = ErrorResponse),
        (status = 415, description = "Content-Type が application/json でない", body = ErrorResponse),
        (status = 422, description = "必須フィールドが無い、または型が不正", body = ErrorResponse),
    )
)]
async fn set_frozen(
    AdminUser(admin): AdminUser,
    State(state): State<AppState>,
    AppPath(target_id): AppPath<i64>,
    AppJson(payload): AppJson<SetFrozenRequest>,
) -> Result<StatusCode, AppError> {
    if payload.frozen && target_id == admin.id {
        return Err(AppError::CannotFreezeUser);
    }

    let mut tx = state.pool.begin().await?;

    let frozen = i64::from(payload.frozen);
    // 状態が変わる行だけを更新する。同じ状態への操作は有効な管理者の数を変えないので、
    // 残数の確認を飛ばす (もともと0人の状態でも、何も変えない操作は受け付けて記録する)。
    let changed = sqlx::query!(
        "UPDATE users SET frozen = ? WHERE id = ? AND email_verified = 1 AND frozen != ?",
        frozen,
        target_id,
        frozen
    )
    .execute(&mut *tx)
    .await?
    .rows_affected()
        > 0;
    if !changed && !auth::verified_user_exists(&mut *tx, target_id).await? {
        return Err(AppError::NotFound);
    }

    if payload.frozen && changed && target_role(&mut tx, target_id).await? == Role::Admin {
        ensure_an_admin_remains(&mut tx, AppError::CannotFreezeUser).await?;
    }

    let action = if payload.frozen {
        audit::Action::Freeze
    } else {
        audit::Action::Unfreeze
    };
    audit::record(&mut *tx, admin.id, target_id, action).await?;

    tx.commit().await?;
    tracing::info!(
        admin_user_id = admin.id,
        target_user_id = target_id,
        frozen = payload.frozen,
        "ユーザーの凍結状態を変更しました"
    );
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SetOcrLimitRequest {
    /// 累計金額 (無料枠) の上限 (円)。`null` (省略も同じ) は設定の既定値 (`ocrDefaultBudgetYen`) に従う。
    /// `-1` は無制限、`0` はもう読み取れない。
    #[schema(example = 80)]
    ocr_budget_yen: Option<i64>,
}

/// ユーザーごとの OCR 累計金額の上限を変更する (docs/authentication.md)。
///
/// 自分自身にも設定できる。締め出しにつながる凍結と違い、上限を絞っても管理操作は続けられる。
#[utoipa::path(
    put,
    path = "/admin/users/{id}/ocr-limit",
    params(("id" = i64, Path, description = "対象ユーザーの id")),
    request_body = SetOcrLimitRequest,
    responses(
        (status = 204, description = "変更した"),
        (status = 400, description = "リクエストボディが不正 (JSON として解釈できない等)、または id がパスパラメータとして解釈できない (数値でない等)", body = ErrorResponse),
        (status = 401, description = "未ログイン", body = ErrorResponse),
        (status = 403, description = "管理者ではない", body = ErrorResponse),
        (status = 404, description = "対象のユーザーが存在しない", body = ErrorResponse),
        (status = 415, description = "Content-Type が application/json でない", body = ErrorResponse),
        (status = 422, description = "上限が値域外", body = ErrorResponse),
    )
)]
async fn set_ocr_limit(
    AdminUser(admin): AdminUser,
    State(state): State<AppState>,
    AppPath(target_id): AppPath<i64>,
    AppJson(payload): AppJson<SetOcrLimitRequest>,
) -> Result<StatusCode, AppError> {
    if let Some(budget) = payload.ocr_budget_yen
        && !validation::is_valid_ocr_budget_yen(budget)
    {
        return Err(AppError::Validation(format!(
            "ocrBudgetYen must be null, {} (unlimited), or within {}..={}",
            validation::OCR_BUDGET_YEN_UNLIMITED,
            validation::OCR_BUDGET_YEN_RANGE.start(),
            validation::OCR_BUDGET_YEN_RANGE.end()
        )));
    }

    let mut tx = state.pool.begin().await?;

    let updated = sqlx::query!(
        "UPDATE users SET ocr_budget_yen = ? WHERE id = ? AND email_verified = 1",
        payload.ocr_budget_yen,
        target_id
    )
    .execute(&mut *tx)
    .await?;
    if updated.rows_affected() == 0 {
        return Err(AppError::NotFound);
    }

    audit::record(&mut *tx, admin.id, target_id, audit::Action::ChangeOcrLimit).await?;

    tx.commit().await?;
    tracing::info!(
        admin_user_id = admin.id,
        target_user_id = target_id,
        ocr_budget_yen = ?payload.ocr_budget_yen,
        "ユーザーの OCR 上限を変更しました"
    );
    Ok(StatusCode::NO_CONTENT)
}

/// ユーザーの削除を予約する (docs/authentication.md)。本人による削除と同じフローを通るが、
/// 起点を管理者として記録するため、本人のログインでは取り消されない (取り消しは
/// `DELETE /admin/users/{id}/deletion`)。既に予約済みなら予定日も起点も変わらない。
///
/// 予約できないのは凍結と同じ2つ。削除は予約した時点で「有効な管理者」から外れるため、
/// 凍結と同じく誰も管理できない状態を作りうる。
///
/// - 自分自身 (管理画面から自分を消せてしまうのは事故のもと。本人の削除は設定画面にある)
/// - 有効な管理者が残らなくなる操作。判定と更新を同じトランザクションに入れて防ぐ
#[utoipa::path(
    post,
    path = "/admin/users/{id}/deletion",
    params(("id" = i64, Path, description = "対象ユーザーの id")),
    responses(
        (status = 200, description = "削除を予約した", body = DeletionResponse),
        (status = 400, description = "id がパスパラメータとして解釈できない (数値でない等)", body = ErrorResponse),
        (status = 401, description = "未ログイン", body = ErrorResponse),
        (status = 403, description = "管理者ではない", body = ErrorResponse),
        (status = 404, description = "対象のユーザーが存在しない", body = ErrorResponse),
        (status = 409, description = "自分自身、または有効な管理者が残らなくなる削除", body = ErrorResponse),
    )
)]
async fn schedule_user_deletion(
    AdminUser(admin): AdminUser,
    State(state): State<AppState>,
    AppPath(target_id): AppPath<i64>,
) -> Result<Json<DeletionResponse>, AppError> {
    if target_id == admin.id {
        return Err(AppError::CannotDeleteUser);
    }

    let mut tx = state.pool.begin().await?;

    let scheduled = account::schedule_deletion(&mut tx, target_id, DeletionOrigin::Admin)
        .await?
        .ok_or(AppError::NotFound)?;

    // 予約済みへの再操作は有効な管理者の数を変えないので、残数を確かめない (凍結と同じ)。
    if scheduled.newly_scheduled && target_role(&mut tx, target_id).await? == Role::Admin {
        ensure_an_admin_remains(&mut tx, AppError::CannotDeleteUser).await?;
    }

    audit::record(&mut *tx, admin.id, target_id, audit::Action::Delete).await?;

    tx.commit().await?;
    tracing::info!(
        admin_user_id = admin.id,
        target_user_id = target_id,
        newly_scheduled = scheduled.newly_scheduled,
        "ユーザーの削除を予約しました"
    );
    Ok(Json(DeletionResponse {
        deletion_scheduled_at: scheduled.scheduled_at,
        grace_days: account::DELETION_GRACE_DAYS,
    }))
}

/// 管理者が起点の削除予約を取り消す (docs/authentication.md)。
///
/// 本人が起点の予約は取り消さない (409)。予約が無ければ何もせず 204 を返す (監査ログには残す)。
#[utoipa::path(
    delete,
    path = "/admin/users/{id}/deletion",
    params(("id" = i64, Path, description = "対象ユーザーの id")),
    responses(
        (status = 204, description = "取り消した (予約が無かった場合も含む)"),
        (status = 400, description = "id がパスパラメータとして解釈できない (数値でない等)", body = ErrorResponse),
        (status = 401, description = "未ログイン", body = ErrorResponse),
        (status = 403, description = "管理者ではない", body = ErrorResponse),
        (status = 404, description = "対象のユーザーが存在しない", body = ErrorResponse),
        (status = 409, description = "本人が起点の削除予約", body = ErrorResponse),
    )
)]
async fn cancel_user_deletion(
    AdminUser(admin): AdminUser,
    State(state): State<AppState>,
    AppPath(target_id): AppPath<i64>,
) -> Result<StatusCode, AppError> {
    let mut tx = state.pool.begin().await?;

    let outcome = account::cancel_admin_deletion(&mut tx, target_id).await?;
    match outcome {
        AdminDeletionCancel::Cancelled | AdminDeletionCancel::NotScheduled => {}
        AdminDeletionCancel::ScheduledByUser => return Err(AppError::CannotCancelDeletion),
        AdminDeletionCancel::NotFound => return Err(AppError::NotFound),
    }

    audit::record(&mut *tx, admin.id, target_id, audit::Action::CancelDeletion).await?;

    tx.commit().await?;
    tracing::info!(
        admin_user_id = admin.id,
        target_user_id = target_id,
        cancelled = outcome == AdminDeletionCancel::Cancelled,
        "ユーザーの削除予約を取り消しました"
    );
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CreateUserRequest {
    #[schema(example = "kyoko")]
    username: String,
    #[schema(example = "password")]
    password: String,
    /// 省略時は一般ユーザー。
    #[serde(default)]
    role: Option<Role>,
}

/// 管理画面から作ったユーザー。ユーザーIDは保存される形 (小文字) を返す。
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CreateUserResponse {
    id: i64,
    #[schema(example = "kyoko")]
    username: String,
}

/// ユーザーを作る (docs/authentication.md)。`--create-user` CLI と同じ経路
/// (`auth::create_user`) を通すため、作られるユーザーは常にメール確認済みになる。
/// 管理者が本人を確かめて登録する前提の機能なので、これでよい。
#[utoipa::path(
    post,
    path = "/admin/users",
    request_body = CreateUserRequest,
    responses(
        (status = 201, description = "作成した", body = CreateUserResponse),
        (status = 400, description = "リクエストボディが不正 (JSON として解釈できない等)", body = ErrorResponse),
        (status = 401, description = "未ログイン", body = ErrorResponse),
        (status = 403, description = "管理者ではない", body = ErrorResponse),
        (status = 409, description = "ユーザーIDが既に使われている", body = ErrorResponse),
        (status = 415, description = "Content-Type が application/json でない", body = ErrorResponse),
        (status = 422, description = "ユーザーIDが不正、またはパスワードが設定のポリシーを満たさない", body = ErrorResponse),
    )
)]
async fn create_user(
    AdminUser(admin): AdminUser,
    State(state): State<AppState>,
    AppJson(payload): AppJson<CreateUserRequest>,
) -> Result<(StatusCode, Json<CreateUserResponse>), AppError> {
    let (username, hash) =
        super::validate_new_user(&state, &payload.username, payload.password).await?;
    let role = payload.role.unwrap_or(Role::User);

    let mut tx = state.pool.begin().await?;
    auth::discard_unverified_user(&mut *tx, &username).await?;
    let id = match auth::create_user(&mut *tx, &username, NewPassword::Usable(&hash), role).await {
        Ok(id) => id,
        // UNIQUE 制約違反だけは、状態との衝突として 409 に振り分ける。
        Err(sqlx::Error::Database(err)) if err.is_unique_violation() => {
            return Err(AppError::UsernameTaken);
        }
        Err(err) => return Err(err.into()),
    };

    audit::record(&mut *tx, admin.id, id, audit::Action::Create).await?;

    tx.commit().await?;
    tracing::info!(
        admin_user_id = admin.id,
        target_user_id = id,
        ?role,
        "ユーザーを作成しました"
    );
    Ok((
        StatusCode::CREATED,
        Json(CreateUserResponse { id, username }),
    ))
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ResetPasswordRequest {
    #[schema(example = "new-password")]
    password: String,
}

/// ユーザーのパスワードを再設定する (docs/authentication.md)。
///
/// 対象のセッションはすべて断たれる (docs/authentication.md)。本人による変更と違い操作しているのは別人なので、
/// 生き残らせるセッションが無い。
///
/// 再設定できないのは次の2つ。
///
/// - 自分自身。本人の変更 (`PUT /account/password`) は現在のパスワードの照合を通すので、
///   セッションだけを奪った相手にこちらを迂回路として使わせない
/// - パスワードを持たないユーザー (Google 専用アカウント)。新しく持たせるのは、同じく
///   セッションだけを奪った相手に ID/PW ログインの経路を作らせないため本人のメール確認が
///   前提で、管理者が起点の再設定とは別の機能になる
#[utoipa::path(
    put,
    path = "/admin/users/{id}/password",
    params(("id" = i64, Path, description = "対象ユーザーの id")),
    request_body = ResetPasswordRequest,
    responses(
        (status = 204, description = "再設定した"),
        (status = 400, description = "リクエストボディが不正 (JSON として解釈できない等)、または id がパスパラメータとして解釈できない (数値でない等)", body = ErrorResponse),
        (status = 401, description = "未ログイン", body = ErrorResponse),
        (status = 403, description = "管理者ではない", body = ErrorResponse),
        (status = 404, description = "対象のユーザーが存在しない", body = ErrorResponse),
        (status = 409, description = "自分自身、または対象がパスワードを持たない (Google 専用アカウント)", body = ErrorResponse),
        (status = 415, description = "Content-Type が application/json でない", body = ErrorResponse),
        (status = 422, description = "パスワードが設定のポリシーを満たさない", body = ErrorResponse),
    )
)]
async fn reset_password(
    AdminUser(admin): AdminUser,
    State(state): State<AppState>,
    AppPath(target_id): AppPath<i64>,
    AppJson(payload): AppJson<ResetPasswordRequest>,
) -> Result<StatusCode, AppError> {
    if target_id == admin.id {
        return Err(AppError::CannotResetOwnPassword);
    }
    auth::validate_password(&state.password, &payload.password)?;

    // ハッシュ化の間 DB を塞がないよう、トランザクションを開く前に済ませる。
    let hash = auth::hash_password_async(payload.password).await?;

    let mut tx = state.pool.begin().await?;
    match auth::reset_password_hash(&mut tx, target_id, &hash).await? {
        auth::PasswordReset::Done => {}
        auth::PasswordReset::NoPassword => return Err(AppError::CannotResetPassword),
        auth::PasswordReset::NotFound => return Err(AppError::NotFound),
    }

    audit::record(&mut *tx, admin.id, target_id, audit::Action::ResetPassword).await?;

    tx.commit().await?;
    tracing::info!(
        admin_user_id = admin.id,
        target_user_id = target_id,
        "ユーザーのパスワードを再設定しました"
    );
    password_notice::spawn(&state, target_id, password_notice::Origin::Admin);
    Ok(StatusCode::NO_CONTENT)
}

pub fn router() -> OpenApiRouter<AppState> {
    OpenApiRouter::new()
        .routes(routes!(list_users, create_user))
        .routes(routes!(get_user))
        .routes(routes!(list_user_records))
        .routes(routes!(reset_password))
        .routes(routes!(set_frozen))
        .routes(routes!(set_ocr_limit))
        .routes(routes!(schedule_user_deletion, cancel_user_deletion))
}
