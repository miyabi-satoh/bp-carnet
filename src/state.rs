use std::sync::Arc;

use sqlx::SqlitePool;

use crate::app_login_nonce::NonceStore;
use crate::apple_login::AppleLoginClient;
use crate::auth::AttemptRateLimiter;
use crate::config::PasswordConfig;
use crate::google_login::GoogleLoginClient;
use crate::line_login::LineLoginClient;
use crate::mail::Mailer;
use crate::ocr::OcrService;
use crate::payments::app_store::AppStoreClient;
use crate::payments::stripe::StripeClient;

/// axum ハンドラ間で共有するアプリケーション状態。
/// `SqlitePool` は内部で `Arc` を持つため、`Clone` しても実体は共有される。
#[derive(Clone)]
pub struct AppState {
    pub pool: SqlitePool,
    /// ログイン試行の「接続元 + ユーザー名」単位の制限 (docs/authentication.md、接続元は
    /// `forwarded::rate_limit_key`)。接続元が分からないときはユーザー名単位で数える。
    pub login_rate_limiter: Arc<AttemptRateLimiter>,
    /// ログイン試行の接続元単位 (`forwarded::rate_limit_key`) の制限。名前を変えながら試す相手を止める。
    pub login_client_rate_limiter: Arc<AttemptRateLimiter>,
    /// パスワード変更での現在のパスワードの照合試行。ログインとは別の実体にして、
    /// 一方への試行がもう一方を止めないようにする。
    pub password_change_rate_limiter: Arc<AttemptRateLimiter>,
    /// メールを送らせる要求 (サインアップ・パスワードリセット) の、接続元単位の制限
    /// (docs/architecture.md)。キーは `forwarded::rate_limit_key` (IPv4 はアドレス、IPv6 は /64)。
    pub mail_request_rate_limiter: Arc<AttemptRateLimiter>,
    /// メールのリンクの先でパスワードを設定する要求 (サインアップの確認完了・パスワードの再設定)
    /// のうち、Argon2 まで進むものの、接続元単位 (`forwarded::rate_limit_key`、IPv6 は /64) の制限。トークン1つにつき
    /// Argon2 は1回だが、溜めたトークンを一度に使って Argon2 を集中させる経路を塞ぐため、全体の
    /// 上限も効かせる。使えないトークンの要求は数えない (`api::email_link::consume_for_new_password`)。
    pub link_password_rate_limiter: Arc<AttemptRateLimiter>,
    /// アプリの LINE・Apple ログインの `nonce` (docs/mobile-app.md)。
    pub app_login_nonces: Arc<NonceStore>,
    /// `nonce` の発行の、接続元単位 (`forwarded::rate_limit_key`) の制限。発行は未認証で叩けるため、
    /// 一つの接続元が [`NonceStore`] の上限を埋めて、ほかの人のログインを止められないようにする。
    pub app_nonce_rate_limiter: Arc<AttemptRateLimiter>,
    pub mailer: Arc<Mailer>,
    pub ocr: Arc<OcrService>,
    /// OCR の累計金額の既定の上限 (円。`crate::config::OcrCosts`)。ユーザーごとの上限
    /// (`users.ocr_budget_yen`) が無いときに使う。負値は無制限。
    pub ocr_free_budget_yen: i64,
    /// 読み取りの最中のユーザー (同じユーザーの読み取りを1件ずつにする)。
    pub ocr_in_flight: Arc<crate::ocr_usage::InFlight>,
    /// 使った額の計算に使う単価。
    pub ocr_pricing: crate::config::OcrPricing,
    /// 買い足し1回で付ける量 (1/1000円)。
    pub ocr_topup_grant_milli_yen: i64,
    /// OCR 枠買い足し (→ docs/payments.md) の Checkout 作成・Webhook 検証に使う。
    /// `enabled() == false` なら設定未完了として買い足し導線ごと隠す。
    pub stripe: Arc<StripeClient>,
    /// アプリ内課金 (→ docs/payments.md) の取引の確かめと通知の受け取りに使う。
    /// `enabled() == false` ならアプリでの買い足しを出さない。
    pub app_store: Arc<AppStoreClient>,
    /// アプリ内課金の取引の確かめの回数の制限 (ユーザー単位)。
    pub app_store_transaction_rate_limiter: Arc<AttemptRateLimiter>,
    pub google_login: Arc<GoogleLoginClient>,
    pub line_login: Arc<LineLoginClient>,
    pub apple_login: Arc<AppleLoginClient>,
    /// Google・LINE・Apple の state/PKCE 専用 Cookie (`crate::oauth_cookie`) の `Secure` 属性判定に使う。
    /// セッション Cookie と同じ `config.session.secure_cookie` の値をそのまま持つ。
    pub secure_cookie: bool,
    /// セッションとアプリのトークン (`crate::app_token`) を失効させる、最終利用からの日数
    /// (`config.session.expiry_days`)。
    pub session_expiry_days: i64,
    /// パスワードの最小強度 (`config.toml` の `[password]`)。パスワードを設定する API と、
    /// 入力欄に条件を示す frontend 向けの `GET /auth/password-policy` が使う。
    pub password: Arc<PasswordConfig>,
    /// 「オープンβテスト中」の表示を出すか (`config.toml` の `[server] beta_notice`)。
    pub beta_notice: bool,
    /// 問い合わせ先の URL (`config.toml` の `[server] contact_url`)。
    pub contact_url: Arc<str>,
    /// 紹介ページの URL (`config.toml` の `[server] intro_url`)。空なら出さない。
    pub intro_url: Arc<str>,
    /// 公開の URL (`config.toml` の `[server] public_url`、末尾の `/` なし)。
    /// canonical・sitemap.xml・robots.txt の絶対 URL に使う (docs/architecture.md)。
    pub public_url: Arc<str>,
}
