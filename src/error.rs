//! API 全体で共通のエラー型とレスポンス形式。

use axum::Json;
use axum::extract::multipart::MultipartRejection;
use axum::extract::rejection::{JsonRejection, PathRejection, QueryRejection};
use axum::extract::{FromRequest, FromRequestParts, Multipart, Path, Query, Request};
use axum::http::StatusCode;
use axum::http::request::Parts;
use axum::response::{IntoResponse, Response};
use serde::Serialize;
use serde::de::DeserializeOwned;
use utoipa::ToSchema;

/// API 全体で共通のエラー型。バリアントは実際にそれを生成する箇所ができた時点で追加する
/// (投機的に増やさない)。
#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("not found")]
    NotFound,
    #[error("database error")]
    Database(#[from] sqlx::Error),
    /// ログイン前 / セッション切れ。「間違ったパスワード」とは別コードにして、
    /// frontend が「ログイン画面に誘導する」か「パスワードが違うと伝える」かを区別できるようにする。
    #[error("unauthorized")]
    Unauthorized,
    #[error("invalid credentials")]
    InvalidCredentials,
    /// パスワード変更で、現在のパスワードが一致しなかった。`InvalidCredentials` (401) と分けるのは、
    /// frontend が 401 をセッション切れとしてログイン画面へ誘導するため (ログイン中の操作なので、
    /// ログイン画面へ送るのは誤り)。
    #[error("current password is incorrect")]
    IncorrectPassword,
    /// ログイン済みだが権限が足りない (管理者APIを一般ユーザーが呼んだ)。未ログインの
    /// `Unauthorized` と分けるのは、frontend が 401 をセッション切れとして扱うため。
    #[error("forbidden")]
    Forbidden,
    /// 凍結中のアカウントでログインしようとした (docs/authentication.md)。パスワードの照合を
    /// 通ってから返すため、ユーザー名の存在有無は漏れない。
    #[error("account is frozen")]
    AccountFrozen,
    /// アプリの LINE・Google・Apple のログイン (docs/mobile-app.md) を、この構成では受け付けない。
    #[error("the login provider is not configured")]
    LoginProviderDisabled,
    /// アプリの LINE・Google・Apple のログインで、本人の確認が取れなかった (提供元が拒否した・
    /// 提供元に届かない・メールアドレスが確かめられていない等)。原因はログに残す。
    #[error("external login failed")]
    ExternalLoginFailed,
    /// アプリの LINE ログインで、同意画面のメールアドレスの提供をオフにされた (docs/authentication.md)。
    /// ウェブの `line_email_required` と同じく、許可するよう案内する。
    #[error("LINE did not provide the email address")]
    LineEmailRequired,
    /// アプリの Google ログインで、メールアドレスを Google が持ち主として保証していない
    /// (Gmail でも Google Workspace でもない)。ウェブの `google_email_untrusted` と同じく、
    /// メールアドレスで入るよう案内する。
    #[error("Google does not vouch for the email address")]
    GoogleEmailUntrusted,
    /// アプリの LINE・Google・Apple のログインで、同じメールアドレスのアカウントに紐付けられない
    /// (同じ提供元の別のアカウントと連携済み)。
    #[error("the account cannot be linked to the external login")]
    ExternalAccountConflict,
    /// 凍結すると管理できる者がいなくなる操作 (自分自身・最後の有効な管理者)。リクエスト自体は
    /// 妥当で、今の状態と衝突するだけなので 409。
    #[error("cannot freeze this user")]
    CannotFreezeUser,
    /// 削除すると管理できる者がいなくなる操作 (自分自身・最後の有効な管理者)。`CannotFreezeUser`
    /// と条件は同じだが、画面に出す文言が違うためコードを分ける。
    #[error("cannot delete this user")]
    CannotDeleteUser,
    /// 本人が起点の削除予約を、管理者が取り消そうとした (docs/authentication.md)。リクエスト自体は
    /// 妥当で、今の状態と衝突するだけなので 409。
    #[error("cannot cancel a deletion requested by the user")]
    CannotCancelDeletion,
    /// 作ろうとしたユーザーIDが既に使われている。リクエスト自体は妥当で、今の状態と
    /// 衝突するだけなので 409。
    #[error("username is already taken")]
    UsernameTaken,
    /// 直す・消す・取り込む記録が、画面を開いた後にほかで変わっていた。
    /// 送られた版番号と今の記録が食い違うだけなので 409。
    #[error("the record has been changed elsewhere")]
    RecordConflict,
    /// パスワードを持たないユーザー (Google 専用アカウント) に、管理者がパスワードを
    /// 再設定しようとした。リクエスト自体は妥当で、今の状態と衝突するだけなので 409。
    #[error("this user has no password to reset")]
    CannotResetPassword,
    /// 管理者が自分自身のパスワードを管理APIから再設定しようとした。本人の変更
    /// (`PUT /account/password`) は現在のパスワードの照合を通すため、こちらを迂回路に
    /// させない。`CannotResetPassword` と条件は違うが、どちらも状態との衝突なので 409。
    #[error("cannot reset your own password from the admin API")]
    CannotResetOwnPassword,
    /// 外すとログインできる方法が無くなる連携の解除 (docs/authentication.md)。リクエスト自体は
    /// 妥当で、今の状態と衝突するだけなので 409。
    #[error("cannot unlink the last login method")]
    CannotUnlinkLastLoginMethod,
    #[error("too many requests")]
    TooManyRequests,
    /// メール内リンクのトークンが未知・期限切れ・使用済み。どれかを区別しない (有効だった
    /// トークンかどうかを推測させないため)。
    #[error("the link is invalid or has expired")]
    InvalidEmailToken,
    /// OCR の累計金額の上限 (無料枠) を使い切った (docs/ocr.md)。
    #[error("ocr budget exhausted")]
    OcrBudgetExhausted,
    /// 同じユーザーの読み取りがまだ終わっていない (docs/ocr.md)。
    /// 待てば読めるので、使い切りとは別のコードにする。
    #[error("ocr already in progress")]
    OcrInProgress,
    /// リクエストボディが不正 (JSON として解釈できない・必須フィールド欠落・
    /// `Content-Type` が `application/json` でない等)。`axum::Json` の rejection を
    /// そのまま返すと共通 envelope にならないため、`AppJson` 経由でここに変換する。
    #[error("{message}")]
    InvalidJson { status: StatusCode, message: String },
    /// パスパラメータが不正 (`/records/{id}` の `id` が数値として解釈できない等)。
    /// `axum::extract::Path` の rejection をそのまま返すと `text/plain` になり共通 envelope
    /// にならないため、`AppPath` 経由でここに変換する。
    #[error("{message}")]
    InvalidPath { status: StatusCode, message: String },
    /// クエリパラメータが不正。`axum::extract::Query` の rejection をそのまま返すと
    /// `text/plain` になり共通 envelope にならないため、`AppQuery` 経由でここに変換する。
    #[error("{message}")]
    InvalidQuery { status: StatusCode, message: String },
    /// GET 以外のリクエストで `Origin`/`Referer` がリクエスト自身のオリジンと一致しなかった
    /// (CSRF対策)。`crate::csrf::same_origin_check` ミドルウェアが生成する。
    #[error("cross-site request rejected")]
    CsrfRejected,
    /// リクエストは JSON として妥当だが、値がドメイン上のバリデーションを満たさない場合
    /// (血圧の値域外・収縮期が拡張期以下 等)。JSON 構文自体の不正 (`InvalidJson`) とは区別する。
    #[error("{0}")]
    Validation(String),
    /// パスワードハッシュの検証自体が失敗した (DB 内のハッシュが壊れている等)、
    /// セッションストアの失敗など。クライアント起因ではないため 5xx。
    #[error("internal error")]
    Internal(#[from] crate::auth::Error),
    /// DB に保存されているタイムゾーン名が tzdb に無い。DB の直接編集や tzdb の更新に伴う
    /// ゾーンの廃止で起きうる。リクエスト内容とは無関係なので 5xx。
    #[error("unknown timezone: {0}")]
    UnknownTimezone(String),
    /// DB に保存されている測定日時が解釈できない。書き込み時に正規化しているため、DB を直接
    /// 編集した場合だけ起きる。リクエスト内容とは無関係なので 5xx。
    #[error("invalid stored datetime: {0}")]
    InvalidStoredDatetime(String),
    /// `multipart/form-data` のボディが不正 (境界不正・フィールド欠落等)。
    /// `axum::extract::Multipart` の rejection をそのまま返すと `text/plain` になり
    /// 共通 envelope にならないため、`AppMultipart` 経由でここに変換する。
    #[error("{message}")]
    InvalidMultipart { status: StatusCode, message: String },
    /// アプリの版が古く、今の API と合わない (`crate::app_version`)。アプリは更新を促す画面を出す。
    #[error("this app version is no longer supported")]
    AppUpdateRequired,
    /// 写真を Gemini API に送ることに同意していない (docs/ocr.md)。
    #[error("consent to send photos to Gemini API is required")]
    OcrConsentRequired,
    /// `GEMINI_API_KEY` が未設定で OCR 機能が無効化されている。
    #[error("OCR is not configured on this server")]
    OcrDisabled,
    /// アップロード画像が上限 (10MB) を超えている。
    #[error("image exceeds 10 MB")]
    OcrImageTooLarge,
    /// アップロード画像の MIME タイプが対応外。
    #[error("{0}")]
    OcrUnsupportedMimeType(String),
    /// Gemini API 呼び出し自体、またはそのレスポンスの解釈に失敗した。
    #[error("{message}")]
    OcrUpstream {
        message: String,
        /// ログに原因までつなぐため。`message` はクライアントに返すので、原因の詳細は入れない。
        #[source]
        cause: Option<crate::ocr::OcrError>,
    },
    /// 一括登録・一括置換 (`POST /records/import` 等) で、1件以上の行が値域・整合性
    /// チェックに違反した。通常の `Validation` (常に1件のみ) と異なり、不正な行をすべて `errors` に集めて返す
    /// (ユーザーがどの行を直せばよいか一度で分かるようにするため。PoC は最初の不正行で
    /// 打ち切っていたが、その改善として位置付ける)。
    #[error("{} of {total} row(s) are invalid", errors.len())]
    BatchValidation { total: usize, errors: Vec<RowError> },
    /// Stripe 決済 (OCR 枠買い足し) の設定 (env var・`[payments.stripe] enabled`) が
    /// そろっていない。
    #[error("payments are not configured on this server")]
    PaymentsDisabled,
    /// Webhook の `Stripe-Signature` ヘッダーの検証に失敗した (署名不一致・タイムスタンプが
    /// 許容誤差外)。Stripe からの正規のリクエストではない可能性が高い。
    #[error("stripe webhook signature is invalid")]
    StripeWebhookInvalidSignature,
    /// Stripe API 呼び出し自体、またはそのレスポンスの解釈に失敗した。
    #[error("{message}")]
    PaymentsUpstream {
        message: String,
        /// ログに原因までつなぐため。`message` はクライアントに返すので、原因の詳細は入れない。
        #[source]
        cause: Option<crate::payments::stripe::StripeError>,
    },
    /// App Store Server API で取引を確かめられなかった (通信・Apple の失敗・見つからない等)。
    /// 時間をおけば確かめられうるので、アプリは取引を終えず、通知は再送させる (503)。
    #[error("app store is temporarily unavailable")]
    AppStoreUnavailable(#[source] crate::payments::app_store::AppStoreError),
}

/// 設定のパスワードポリシー違反は、値が規則を満たさないという意味で他のドメイン検証と同じ
/// 扱いにする (422 / `validation_error`)。
impl From<crate::auth::PasswordPolicyError> for AppError {
    fn from(err: crate::auth::PasswordPolicyError) -> Self {
        Self::Validation(err.to_string())
    }
}

/// バッチ処理で 1 行の検証が失敗したことを表す。`index` はリクエスト配列内の 0 始まりの位置。
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct RowError {
    pub index: usize,
    pub message: String,
}

impl AppError {
    fn status_and_code(&self) -> (StatusCode, &'static str) {
        match self {
            Self::NotFound => (StatusCode::NOT_FOUND, "not_found"),
            Self::Database(_) => (StatusCode::SERVICE_UNAVAILABLE, "database_unavailable"),
            Self::Unauthorized => (StatusCode::UNAUTHORIZED, "unauthorized"),
            Self::InvalidCredentials => (StatusCode::UNAUTHORIZED, "invalid_credentials"),
            Self::IncorrectPassword => (StatusCode::UNPROCESSABLE_ENTITY, "incorrect_password"),
            Self::Forbidden => (StatusCode::FORBIDDEN, "forbidden"),
            Self::AccountFrozen => (StatusCode::FORBIDDEN, "account_frozen"),
            Self::LoginProviderDisabled => {
                (StatusCode::SERVICE_UNAVAILABLE, "login_provider_disabled")
            }
            Self::ExternalLoginFailed => (StatusCode::UNAUTHORIZED, "external_login_failed"),
            Self::LineEmailRequired => (StatusCode::UNAUTHORIZED, "line_email_required"),
            Self::GoogleEmailUntrusted => (StatusCode::UNAUTHORIZED, "google_email_untrusted"),
            Self::ExternalAccountConflict => (StatusCode::CONFLICT, "external_account_conflict"),
            Self::CannotFreezeUser => (StatusCode::CONFLICT, "cannot_freeze_user"),
            Self::CannotDeleteUser => (StatusCode::CONFLICT, "cannot_delete_user"),
            Self::CannotCancelDeletion => (StatusCode::CONFLICT, "cannot_cancel_deletion"),
            Self::UsernameTaken => (StatusCode::CONFLICT, "username_taken"),
            Self::RecordConflict => (StatusCode::CONFLICT, "record_conflict"),
            Self::CannotResetPassword => (StatusCode::CONFLICT, "cannot_reset_password"),
            Self::CannotResetOwnPassword => (StatusCode::CONFLICT, "cannot_reset_own_password"),
            Self::CannotUnlinkLastLoginMethod => {
                (StatusCode::CONFLICT, "cannot_unlink_last_login_method")
            }
            Self::TooManyRequests => (StatusCode::TOO_MANY_REQUESTS, "too_many_requests"),
            Self::InvalidEmailToken => (StatusCode::BAD_REQUEST, "invalid_email_token"),
            Self::OcrBudgetExhausted => (StatusCode::TOO_MANY_REQUESTS, "ocr_budget_exhausted"),
            Self::OcrInProgress => (StatusCode::CONFLICT, "ocr_in_progress"),
            Self::InvalidJson { status, .. } => (*status, "invalid_request_body"),
            Self::InvalidPath { status, .. } => (*status, "invalid_path"),
            Self::InvalidQuery { status, .. } => (*status, "invalid_query"),
            Self::CsrfRejected => (StatusCode::FORBIDDEN, "csrf_rejected"),
            Self::Validation(_) => (StatusCode::UNPROCESSABLE_ENTITY, "validation_error"),
            Self::Internal(_) => (StatusCode::INTERNAL_SERVER_ERROR, "internal_error"),
            Self::UnknownTimezone(_) => (StatusCode::INTERNAL_SERVER_ERROR, "unknown_timezone"),
            Self::InvalidStoredDatetime(_) => (StatusCode::INTERNAL_SERVER_ERROR, "internal_error"),
            Self::InvalidMultipart { status, .. } => (*status, "invalid_request_body"),
            Self::AppUpdateRequired => (StatusCode::UPGRADE_REQUIRED, "app_update_required"),
            Self::OcrConsentRequired => (StatusCode::FORBIDDEN, "ocr_consent_required"),
            Self::OcrDisabled => (StatusCode::SERVICE_UNAVAILABLE, "ocr_disabled"),
            Self::OcrImageTooLarge => (StatusCode::PAYLOAD_TOO_LARGE, "ocr_image_too_large"),
            Self::OcrUnsupportedMimeType(_) => (
                StatusCode::UNPROCESSABLE_ENTITY,
                "ocr_unsupported_mime_type",
            ),
            Self::OcrUpstream { .. } => (StatusCode::BAD_GATEWAY, "ocr_upstream_error"),
            Self::BatchValidation { .. } => {
                (StatusCode::UNPROCESSABLE_ENTITY, "batch_validation_error")
            }
            Self::PaymentsDisabled => (StatusCode::SERVICE_UNAVAILABLE, "payments_disabled"),
            Self::StripeWebhookInvalidSignature => {
                (StatusCode::BAD_REQUEST, "stripe_webhook_invalid_signature")
            }
            Self::PaymentsUpstream { .. } => (StatusCode::BAD_GATEWAY, "payments_upstream_error"),
            Self::AppStoreUnavailable(_) => {
                (StatusCode::SERVICE_UNAVAILABLE, "app_store_unavailable")
            }
        }
    }
}

/// レスポンスボディの共通 envelope。`code` が機械可読な契約 (frontend はこちらで表示文言を引く)、
/// `message` はローカライズしないデバッグ用の説明文。`rows` は `BatchValidation` のときだけ埋まる。
#[derive(Debug, Serialize, ToSchema)]
pub struct ErrorResponse {
    error: ErrorBody,
}

#[derive(Debug, Serialize, ToSchema)]
struct ErrorBody {
    code: &'static str,
    message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    rows: Option<Vec<RowError>>,
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let (status, code) = self.status_and_code();
        if status.is_server_error() {
            // ログには原因までつなぐ: `Database` バリアントの元の sqlx::Error まで残すため。
            // レスポンスボディ側 (Display) は DB 内部の詳細を漏らさない定型文のまま。
            tracing::error!(error = %crate::error_chain_line(&self), "API エラー");
        }
        let rows = match &self {
            Self::BatchValidation { errors, .. } => Some(errors.clone()),
            _ => None,
        };
        let body = ErrorResponse {
            error: ErrorBody {
                code,
                message: self.to_string(),
                rows,
            },
        };
        (status, Json(body)).into_response()
    }
}

/// `axum::Json` の代わりに使う JSON body エクストラクタ。
/// 抽出失敗 (不正な JSON・必須フィールド欠落・`Content-Type` 不一致) を、素の
/// axum レスポンスではなく `AppError` (共通 envelope) に変換する。
pub struct AppJson<T>(pub T);

impl<S, T> FromRequest<S> for AppJson<T>
where
    S: Send + Sync,
    T: DeserializeOwned,
{
    type Rejection = AppError;

    async fn from_request(req: Request, state: &S) -> Result<Self, Self::Rejection> {
        match Json::<T>::from_request(req, state).await {
            Ok(Json(value)) => Ok(AppJson(value)),
            Err(rejection) => Err(json_rejection_to_app_error(rejection)),
        }
    }
}

fn json_rejection_to_app_error(rejection: JsonRejection) -> AppError {
    AppError::InvalidJson {
        status: rejection.status(),
        message: rejection.body_text(),
    }
}

/// `axum::extract::Path` の代わりに使うパスパラメータエクストラクタ。
/// 抽出失敗 (数値であるべき id が数値でない等) を、素の axum レスポンス
/// (`text/plain`) ではなく `AppError` (共通 envelope) に変換する。
pub struct AppPath<T>(pub T);

impl<S, T> FromRequestParts<S> for AppPath<T>
where
    S: Send + Sync,
    T: serde::de::DeserializeOwned + Send,
{
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        match Path::<T>::from_request_parts(parts, state).await {
            Ok(Path(value)) => Ok(AppPath(value)),
            Err(rejection) => Err(path_rejection_to_app_error(rejection)),
        }
    }
}

fn path_rejection_to_app_error(rejection: PathRejection) -> AppError {
    AppError::InvalidPath {
        status: rejection.status(),
        message: rejection.body_text(),
    }
}

/// `axum::extract::Query` の代わりに使うクエリパラメータエクストラクタ。
/// 抽出失敗を、素の axum レスポンス (`text/plain`) ではなく `AppError` (共通 envelope) に変換する。
pub struct AppQuery<T>(pub T);

impl<S, T> FromRequestParts<S> for AppQuery<T>
where
    S: Send + Sync,
    T: serde::de::DeserializeOwned,
{
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        match Query::<T>::from_request_parts(parts, state).await {
            Ok(Query(value)) => Ok(AppQuery(value)),
            Err(rejection) => Err(query_rejection_to_app_error(rejection)),
        }
    }
}

fn query_rejection_to_app_error(rejection: QueryRejection) -> AppError {
    AppError::InvalidQuery {
        status: rejection.status(),
        message: rejection.body_text(),
    }
}

/// `axum::extract::Multipart` の代わりに使う `multipart/form-data` エクストラクタ。
/// 抽出失敗 (境界不正・`Content-Type` 不一致等) を、素の axum レスポンス
/// (`text/plain`) ではなく `AppError` (共通 envelope) に変換する。
pub struct AppMultipart(pub Multipart);

impl<S> FromRequest<S> for AppMultipart
where
    S: Send + Sync,
{
    type Rejection = AppError;

    async fn from_request(req: Request, state: &S) -> Result<Self, Self::Rejection> {
        match Multipart::from_request(req, state).await {
            Ok(multipart) => Ok(AppMultipart(multipart)),
            Err(rejection) => Err(multipart_rejection_to_app_error(rejection)),
        }
    }
}

fn multipart_rejection_to_app_error(rejection: MultipartRejection) -> AppError {
    AppError::InvalidMultipart {
        status: rejection.status(),
        message: rejection.body_text(),
    }
}
