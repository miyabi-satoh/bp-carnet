// AGENTS.md「コードの規約」: テストは .unwrap() ではなく .expect("失敗理由") を使う。
// レビューだけでは見落としやすいため、テストビルドに限って clippy で警告する (本体コードは対象外)。
#![cfg_attr(test, warn(clippy::unwrap_used))]

// crate の外 (`main.rs`・`tests/`) から使うモジュールだけ `pub` にする (AGENTS.md「コードの規約」)。
pub mod account;
pub(crate) mod api;
pub(crate) mod app_login_nonce;
pub(crate) mod app_token;
pub(crate) mod app_version;
pub mod apple_login;
pub(crate) mod audit;
pub mod auth;
pub mod cli;
pub mod config;
pub(crate) mod csrf;
pub mod db;
pub(crate) mod email_change;
pub mod email_token;
pub(crate) mod error;
pub mod external_login;
pub(crate) mod forwarded;
pub mod google_login;
pub(crate) mod inactivity;
pub(crate) mod jwt;
pub mod line_login;
pub(crate) mod local_time;
pub mod logging;
pub mod mail;
pub(crate) mod oauth_cookie;
pub(crate) mod oauth_identity;
pub mod ocr;
pub(crate) mod ocr_consent;
pub(crate) mod ocr_dump;
pub(crate) mod ocr_usage;
pub(crate) mod password_notice;
pub(crate) mod password_reset;
pub mod payments;
pub(crate) mod seo;
pub mod session;
pub(crate) mod settings;
pub mod signup;
pub mod single_instance;
pub(crate) mod state;
pub(crate) mod static_files;
pub(crate) mod stats;
pub mod terms;
#[cfg(test)]
mod test_support;
pub(crate) mod timezones;
pub(crate) mod token;
pub(crate) mod token_cipher;
pub(crate) mod upstream;
pub(crate) mod validation;

use std::sync::Arc;

use axum::Router;
use axum::http::{HeaderValue, Method, header};
use axum::middleware;
use axum::routing::get;
use sqlx::SqlitePool;
use tower_http::compression::CompressionLayer;
use tower_http::cors::CorsLayer;
use tower_http::trace::TraceLayer;
use tower_sessions::cookie::Key;
use tower_sessions_sqlx_store::SqliteStore;

use crate::app_login_nonce::NonceStore;
use crate::apple_login::AppleLoginClient;
use crate::auth::AttemptRateLimiter;
use crate::config::Config;
use crate::google_login::GoogleLoginClient;
use crate::line_login::LineLoginClient;
use crate::mail::Mailer;
use crate::ocr::OcrService;
use crate::payments::app_store::AppStoreClient;
use crate::payments::stripe::StripeClient;
use crate::state::AppState;

/// エラーとその原因 (source) チェーンを1つの文字列にする。`thiserror` の `#[error(...)]` は
/// 最上位のメッセージしか出さないため、`toml::de::Error` が持つ行番号等の詳細を落とさないように辿る。
pub fn error_chain(err: &(dyn std::error::Error + 'static)) -> String {
    join_error_chain(err, "\n原因: ")
}

/// [`error_chain`] をログの1行に収まる形にしたもの。tracing のエラー欄は `error = %error_chain_line(&err)` で書く
/// (`%err` だと原因が落ち、`?err` の Debug は長く詳しさがまちまちなため)。
pub fn error_chain_line(err: &(dyn std::error::Error + 'static)) -> String {
    join_error_chain(err, " / 原因: ")
}

fn join_error_chain(err: &(dyn std::error::Error + 'static), separator: &str) -> String {
    let mut chain = err.to_string();
    let mut previous = chain.clone();
    let mut source = err.source();
    while let Some(err) = source {
        let message = err.to_string();
        // `sqlx::Error` のように、原因を自分の文言に含めたうえで source としても返すエラーがある。
        // 直前の文言に含まれていれば重ねない。
        if !previous.contains(&message) {
            chain.push_str(separator);
            chain.push_str(&message);
        }
        previous = message;
        source = err.source();
    }
    chain
}

/// 環境変数で有効・無効が決まる外部サービス。統合テストから「無効化」「有効化」を明示的に
/// 切り替えられるよう、`build_app` の外で作って渡す (テストの実行環境に依存させないため)。
pub struct Services {
    pub ocr: OcrService,
    pub google_login: GoogleLoginClient,
    /// 物理削除の定期処理 (連携の取り消し) とも共有するため `Arc` で持つ。
    pub line_login: Arc<LineLoginClient>,
    /// 物理削除の定期処理 (連携の取り消し) とも共有するため `Arc` で持つ。
    pub apple_login: Arc<AppleLoginClient>,
    /// 自動退会の予告の定期処理とも共有するため `Arc` で持つ。
    pub mailer: Arc<Mailer>,
    /// OCR 枠買い足し (→ docs/payments.md) の Stripe クライアント。
    pub stripe: StripeClient,
    /// アプリ内課金 (→ docs/payments.md) の App Store Server API クライアント。
    pub app_store: AppStoreClient,
    /// OCR の無料枠・単価と買い足しの量。有効な部品に要る値がそろっていることを、作る側
    /// (`OcrConfig::costs`) で確かめておく。
    pub ocr_costs: config::OcrCosts,
}

/// アプリケーション全体の `Router` を組み立てる。
/// セッションストア用テーブルの作成もここで行う (セッションストアは DB 接続プールを共有する)。
/// `main.rs` と統合テストの両方から共通で呼べるように公開している。
/// `session_layer`・CSRF (同一オリジン) チェックはルーター全体に掛ける: 静的ファイル配信側も
/// Cookie の発行/更新の対象になるようにするため、また GET 以外は API 以外にも一律で
/// 適用しておいて損が無いため (API だけに絞る積極的な理由が無い)。
pub async fn build_app(
    pool: SqlitePool,
    session_key: Key,
    config: &Config,
    services: Services,
) -> Result<Router, db::Error> {
    let Services {
        ocr,
        google_login,
        line_login,
        apple_login,
        mailer,
        stripe,
        app_store,
        ocr_costs,
    } = services;
    let session_store = SqliteStore::new(pool.clone());
    session_store.migrate().await?;
    let secure_cookie = config.session.secure_cookie;
    let session_layer = session::layer(session_store, session_key, &config.session);

    let state = AppState {
        pool,
        login_rate_limiter: Arc::new(AttemptRateLimiter::for_logins()),
        login_client_rate_limiter: Arc::new(AttemptRateLimiter::for_login_clients()),
        password_change_rate_limiter: Arc::new(AttemptRateLimiter::with_defaults()),
        mail_request_rate_limiter: Arc::new(AttemptRateLimiter::for_mail_requests()),
        link_password_rate_limiter: Arc::new(AttemptRateLimiter::with_defaults()),
        app_login_nonces: Arc::new(NonceStore::default()),
        app_nonce_rate_limiter: Arc::new(AttemptRateLimiter::for_app_login_nonces()),
        mailer,
        ocr: Arc::new(ocr),
        ocr_free_budget_yen: ocr_costs.free_budget_yen,
        ocr_in_flight: Arc::default(),
        ocr_pricing: ocr_costs.pricing,
        ocr_topup_grant_milli_yen: ocr_costs.topup_grant_milli_yen,
        stripe: Arc::new(stripe),
        app_store: Arc::new(app_store),
        app_store_transaction_rate_limiter: Arc::new(
            AttemptRateLimiter::for_app_store_transactions(),
        ),
        google_login: Arc::new(google_login),
        line_login,
        apple_login,
        secure_cookie,
        session_expiry_days: config.session.expiry_days,
        password: Arc::new(config.password.clone()),
        beta_notice: config.server.beta_notice,
        contact_url: Arc::from(config.server.contact_url.as_str()),
        intro_url: Arc::from(config.server.intro_url.as_str()),
        public_url: Arc::from(config.server.public_url()),
    };

    let (api_router, _openapi) = api::router().split_for_parts();
    let app = api_router
        .route("/sitemap.xml", get(static_files::sitemap))
        .route("/robots.txt", get(static_files::robots))
        .fallback(static_files::handler)
        // リクエストのログにはクエリを含めずパスだけを残す。
        .layer(TraceLayer::new_for_http().make_span_with(
            |request: &axum::http::Request<axum::body::Body>| {
                tracing::debug_span!(
                    "request",
                    method = %request.method(),
                    path = %request.uri().path(),
                    version = ?request.version(),
                )
            },
        ))
        .layer(CompressionLayer::new())
        .layer(session_layer)
        .layer(middleware::from_fn(session::secure_host_prefixed_cookies))
        // セッションストアに触れる前に、別オリジンからのリクエストを弾く。
        .layer(middleware::from_fn(csrf::same_origin_check))
        // CORS より内側に置き、断りの応答にもアプリが読めるよう CORS のヘッダーを付ける。
        .layer(middleware::from_fn_with_state(
            config.mobile_app.min_version.clone(),
            app_version::require_supported,
        ))
        // CSRF チェックより外側に置き、モバイルアプリの preflight (OPTIONS) に先に答える。
        // 許すのはアプリの出どころだけで、Cookie は許さない (docs/mobile-app.md)。
        .layer(
            CorsLayer::new()
                .allow_origin(HeaderValue::from_static(app_token::APP_ORIGIN))
                .allow_methods([
                    Method::GET,
                    Method::POST,
                    Method::PUT,
                    Method::PATCH,
                    Method::DELETE,
                ])
                .allow_headers([
                    header::AUTHORIZATION,
                    header::CONTENT_TYPE,
                    header::HeaderName::from_static(app_version::HEADER),
                ]),
        )
        // 最後に積む = 最も外側 = リクエストを最初に受ける。CSRF チェックが
        // 信頼済みプロキシの伝えた host/proto を使うため、それより先に解釈しておく。
        .layer(middleware::from_fn_with_state(
            forwarded::ProxySettings {
                trusted_proxies: config.server.trusted_proxies.clone().into(),
                proxy_headers: config.server.proxy_headers,
            },
            forwarded::extract,
        ))
        .with_state(state);

    Ok(app)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, thiserror::Error)]
    #[error("設定ファイルを読めませんでした")]
    struct Wrapping(#[source] std::io::Error);

    #[test]
    fn error_chain_lists_each_source_on_its_own_line() {
        let err = Wrapping(std::io::Error::other("disk full"));
        assert_eq!(
            error_chain(&err),
            "設定ファイルを読めませんでした\n原因: disk full"
        );
    }

    #[test]
    fn error_chain_line_joins_the_sources_on_one_line() {
        let err = Wrapping(std::io::Error::other("disk full"));
        assert_eq!(
            error_chain_line(&err),
            "設定ファイルを読めませんでした / 原因: disk full"
        );
    }

    #[test]
    fn error_chain_skips_a_source_already_in_the_message() {
        let err = sqlx::Error::Io(std::io::Error::other("disk full"));
        assert_eq!(
            error_chain(&err),
            "error communicating with database: disk full"
        );
    }
}
