//! `/api/v1` 配下の HTTP API と OpenAPI ドキュメント。

mod account;
mod admin;
mod app_auth;
mod auth;
mod email_change;
mod email_link;
mod health;
mod ocr;
mod password_reset;
mod payments;
mod records;
mod settings;
mod signup;

use utoipa::openapi::OpenApi;
use utoipa_axum::router::OpenApiRouter;

use crate::error::{AppError, ErrorResponse};
use crate::state::AppState;
use crate::validation::{self, UsernameError};
use records::ExportRecordResponse;

/// `UsernameError` を API のエラーに変換する (管理者による作成・メールでの申し込み)。表示用の文言は
/// フロントエンド側が持つため、`Validation` に集約して開発者向けの説明だけを載せる
/// (`api::settings` と同じ方針)。
fn username_error(error: UsernameError) -> AppError {
    let detail = match error {
        UsernameError::Empty => "username must not be empty".to_string(),
        UsernameError::TooLong => format!(
            "username must be at most {} characters",
            validation::USERNAME_MAX_CHARS
        ),
        UsernameError::InvalidCharacter => {
            "username must not contain whitespace or control characters".to_string()
        }
    };
    AppError::Validation(detail)
}

/// 新しく作るユーザーのユーザーIDとパスワードを検証し、保存する形のユーザーIDとパスワードのハッシュを
/// 返す (管理者による作成)。
async fn validate_new_user(
    state: &AppState,
    username: &str,
    password: String,
) -> Result<(String, String), AppError> {
    // 保存される形で検証する。正規化で変わるのは大文字小文字だけだが、検証と保存で
    // 見ている文字列がずれないようにしておく。
    let username = crate::auth::normalize_username(username);
    validation::validate_username(&username).map_err(username_error)?;
    crate::auth::validate_password(&state.password, &password)?;
    let hash = crate::auth::hash_password_async(password).await?;
    Ok((username, hash))
}

/// OpenAPI ドキュメントのベース (タイトル・スキーマ登録)。
/// `ErrorResponse` はどのハンドラの戻り値にも直接現れない (`AppError` 経由でレスポンスになる)
/// ため、明示的に登録しないと OpenAPI スキーマから漏れる。`ExportRecordResponse` も同様で、
/// エクスポートの本文は CSV テキスト (`body = String`) のため型としては参照されないが、
/// CSV の列名と型を示すドキュメントとして残したいので明示的に登録する。
#[derive(utoipa::OpenApi)]
#[openapi(
    info(title = "bp-carnet"),
    components(schemas(ErrorResponse, ExportRecordResponse))
)]
struct ApiDoc;

/// `/api/v1` を nest した、state 未確定のルーター。
/// `Router<AppState>` はまだ `AppState` の実体 (DB 接続プール) を必要とせず組み立てられる
/// ため、`--openapi` のように DB に触れず OpenAPI ドキュメントだけ欲しい場合はここで止めてよい。
///
/// `fallback` を nest される側に設定しておくのが重要: axum の `nest` はネストするルーター自身が
/// fallback を持つ場合はそれを引き継ぐため、`/api/v1/*` 配下の未知のパスが SPA の `index.html`
/// にフォールバックしてしまうのを防げる。
pub fn router() -> OpenApiRouter<AppState> {
    let v1 = OpenApiRouter::new()
        .merge(health::router())
        .merge(auth::router())
        .merge(app_auth::router())
        .merge(signup::router())
        .merge(password_reset::router())
        .merge(account::router())
        .merge(email_change::router())
        .merge(admin::router())
        .merge(records::router())
        .merge(settings::router())
        .merge(ocr::router())
        .merge(payments::router())
        .fallback(|| async { AppError::NotFound });

    OpenApiRouter::with_openapi(<ApiDoc as utoipa::OpenApi>::openapi()).nest("/api/v1", v1)
}

/// OpenAPI ドキュメントを組み立てる。DB には一切触れない。
pub fn openapi() -> OpenApi {
    let (_router, mut openapi) = router().split_for_parts();
    openapi.info.version = env!("CARGO_PKG_VERSION").to_string();
    // Cargo.toml に description/license を書いていないため、derive が生成した空文字列を
    // 空欄のまま出すよりは省いておく。
    openapi.info.description = None;
    openapi.info.license = None;
    openapi
}

#[cfg(test)]
mod tests {
    use super::*;

    /// operationId はハンドラの関数名から作られる。重なると、frontend の型 (openapi-typescript) が
    /// 別の API の応答を取り違える。
    #[test]
    fn operation_ids_are_unique() {
        let mut seen = std::collections::HashSet::new();
        for (path, item) in openapi().paths.paths {
            for operation in [
                item.get,
                item.put,
                item.post,
                item.delete,
                item.options,
                item.head,
                item.patch,
                item.trace,
            ]
            .into_iter()
            .flatten()
            {
                let id = operation.operation_id.expect("operationId は付くはず");
                assert!(seen.insert(id.clone()), "operationId の重複: {id} ({path})");
            }
        }
    }

    #[test]
    fn openapi_doc_has_prefixed_paths_and_schemas() {
        let openapi = openapi();
        let json = openapi
            .to_json()
            .expect("openapi doc should serialize to json");
        for needle in [
            "/api/v1/health",
            "/api/v1/auth/login",
            "/api/v1/auth/logout",
            "/api/v1/auth/me",
            "/api/v1/auth/providers",
            "/api/v1/account/identities/{provider}",
            "/api/v1/auth/google/login",
            "/api/v1/auth/google/callback",
            "/api/v1/records",
            "/api/v1/records/{id}",
            "/api/v1/records/export",
            "/api/v1/settings",
            "/api/v1/settings/timezones",
            "/api/v1/settings/detected-timezone",
            "/api/v1/ocr",
            "/api/v1/ocr/status",
            "/api/v1/ocr/consent",
            "HealthResponse",
            "ErrorResponse",
            "LoginRequest",
            "UserResponse",
            "AuthProvidersResponse",
            "CreateRecordRequest",
            "RecordResponse",
            "ExportRecordResponse",
            "SettingsResponse",
            "UpdateSettingsRequest",
            "TimezonesResponse",
        ] {
            assert!(json.contains(needle), "missing {needle}: {json}");
        }
        assert!(openapi.info.license.is_none());
        assert!(openapi.servers.is_none());
    }
}
