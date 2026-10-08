//! Google OAuth 2.0 (Authorization Code + PKCE) クライアント。
//!
//! 本人の情報は、トークンエンドポイントが返す ID トークンの中身から読む。署名は検証しない。
//! クライアントシークレットを付けて Google と直接 HTTPS でやり取りして受け取ったものは、Google が
//! 発行したと信頼してよいと Google の資料にあるため (OpenID Connect の「Obtain user information
//! from the ID token」)。userinfo は使わない。メールの持ち主を判断する `hd` を、Google の資料は
//! ID トークンのクレームとしてだけ載せているため。
//!
//! ログイン試行のレートリミット (`crate::auth::AttemptRateLimiter`) には接続しない。
//! ユーザー名キーのレートリミッタに OAuth コールバックを繋ぐと、OAuth 側の乱打で
//! 無関係な ID/PW ユーザーがグローバル上限に巻き込まれてロックされる副作用の方が大きいため、
//! サーバー1台の小規模な運用という既存の割り切りの延長として意図的に接続しない。

use serde::Deserialize;

use crate::config::non_empty_env;
use crate::external_login::ProviderError;
use crate::token::random_url_safe;
use crate::upstream::{http_client, parse_success};

/// ログで呼び出し元を見分けるための名前 (`upstream::success_body`)。
const PROVIDER: &str = "google";

/// コールバックの相対パス (`config.server.public_url` と結合して redirect_uri を作る)。
pub const CALLBACK_PATH: &str = "/api/v1/auth/google/callback";

/// Google OAuth のクライアントID・シークレットの環境変数。機微情報のため config.toml には置かない。
pub const CLIENT_ID_ENV: &str = "GOOGLE_OAUTH_CLIENT_ID";
pub const CLIENT_SECRET_ENV: &str = "GOOGLE_OAUTH_CLIENT_SECRET";

/// Google のエンドポイント。統合テストではモックサーバーに差し替える。
#[derive(Debug, Clone)]
pub struct Endpoints {
    pub authorize: String,
    pub token: String,
}

impl Default for Endpoints {
    fn default() -> Self {
        Self {
            authorize: "https://accounts.google.com/o/oauth2/v2/auth".to_string(),
            token: "https://oauth2.googleapis.com/token".to_string(),
        }
    }
}

struct Credentials {
    client_id: String,
    client_secret: String,
    redirect_uri: String,
}

/// Google OAuth 2.0 でログインするためのクライアント。`enabled() == false` の場合、
/// ハンドラ側はログイン開始そのものを拒否する (`OcrService` の `api_key: None` と同じ設計)。
pub struct GoogleLoginClient {
    credentials: Option<Credentials>,
    endpoints: Endpoints,
    http: reqwest::Client,
}

// client_secret をログに出さないよう手書きする (`OcrService::api_key` と同じ理由)。
impl std::fmt::Debug for GoogleLoginClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GoogleLoginClient")
            .field("enabled", &self.credentials.is_some())
            .finish()
    }
}

impl GoogleLoginClient {
    /// `GOOGLE_OAUTH_CLIENT_ID` / `GOOGLE_OAUTH_CLIENT_SECRET` (どちらも空文字列は未設定扱い) と
    /// `config.server.public_url` から構築する。環境変数のどちらかが欠けていれば無効化された状態になる。
    pub fn from_env(base_url: &str) -> Self {
        let client_id = non_empty_env(CLIENT_ID_ENV);
        let client_secret = non_empty_env(CLIENT_SECRET_ENV);
        let base_url = base_url.trim_end_matches('/');

        let credentials = match (client_id, client_secret) {
            (Some(client_id), Some(client_secret)) => Some(Credentials {
                client_id,
                client_secret,
                redirect_uri: format!("{base_url}{CALLBACK_PATH}"),
            }),
            _ => None,
        };

        Self::new(credentials, Endpoints::default())
    }

    /// 常に無効化されたクライアント。統合テストのように、実行環境の環境変数に依存せず
    /// 「Google ログイン無効時」の挙動を明示的に検証したい場合に使う。
    pub fn disabled() -> Self {
        Self::new(None, Endpoints::default())
    }

    /// 統合テストから、モックサーバーの URL で有効化されたクライアントを作る。
    /// `OcrService::with_api_key` と同じ理由で `#[cfg(test)]` にはしない
    /// (`tests/api.rs` は別クレートとしてコンパイルされるため、`cfg(test)` を付けると
    /// そちらから参照できなくなる)。
    pub fn for_test(
        client_id: impl Into<String>,
        client_secret: impl Into<String>,
        redirect_uri: impl Into<String>,
        endpoints: Endpoints,
    ) -> Self {
        Self::new(
            Some(Credentials {
                client_id: client_id.into(),
                client_secret: client_secret.into(),
                redirect_uri: redirect_uri.into(),
            }),
            endpoints,
        )
    }

    fn new(credentials: Option<Credentials>, endpoints: Endpoints) -> Self {
        Self {
            credentials,
            endpoints,
            http: http_client(),
        }
    }

    pub fn enabled(&self) -> bool {
        self.credentials.is_some()
    }

    /// Google の認可エンドポイントへの完全な URL を組み立てる。`enabled() == false` なら
    /// `None`。`redirect_uri` はコロン・スラッシュを含むため `format!` の手組みではなく
    /// `Url::parse_with_params` を使う (エスケープ漏れ・事故防止)。
    pub fn authorization_url(&self, state: &str, code_challenge: &str) -> Option<String> {
        let creds = self.credentials.as_ref()?;
        let url = reqwest::Url::parse_with_params(
            &self.endpoints.authorize,
            &[
                ("client_id", creds.client_id.as_str()),
                ("redirect_uri", creds.redirect_uri.as_str()),
                ("response_type", "code"),
                ("scope", "openid email profile"),
                ("code_challenge", code_challenge),
                ("code_challenge_method", "S256"),
                ("state", state),
            ],
        )
        .ok()?;
        Some(url.to_string())
    }

    /// 認可コードを ID トークンに交換し、その中身を返す。`code_verifier` は、ウェブのログインで PKCE に
    /// 使ったもの。アプリの Google Sign-In が発行するサーバー用の認可コード (`serverAuthCode`) は、
    /// このクライアント (Web 用) に宛てて発行され、PKCE は掛かっていないので `None` を渡す。
    pub async fn exchange_code(
        &self,
        code: &str,
        code_verifier: Option<&str>,
    ) -> Result<GoogleIdToken, ProviderError> {
        let creds = self.credentials.as_ref().ok_or(ProviderError::Disabled)?;
        let mut form = vec![
            ("grant_type", "authorization_code"),
            ("code", code),
            // アプリの認可コードにも、ウェブと同じ redirect_uri を送る (Google の案内のとおり)。
            ("redirect_uri", creds.redirect_uri.as_str()),
            ("client_id", creds.client_id.as_str()),
            ("client_secret", creds.client_secret.as_str()),
        ];
        if let Some(code_verifier) = code_verifier {
            form.push(("code_verifier", code_verifier));
        }
        let response = self
            .http
            .post(&self.endpoints.token)
            .form(&form)
            .send()
            .await?;
        let token: TokenResponse = parse_success(response, PROVIDER, "token").await?;
        crate::jwt::unverified_payload(&token.id_token).ok_or(ProviderError::ParseResponse)
    }
}

#[derive(Debug, Deserialize)]
struct TokenResponse {
    id_token: String,
}

/// Google の ID トークンのクレーム。使わないものは受け取らない。
#[derive(Debug, Deserialize)]
pub struct GoogleIdToken {
    pub sub: String,
    pub email: Option<String>,
    #[serde(default)]
    pub email_verified: bool,
    /// Google Workspace (Cloud の組織) のアカウントのドメイン。それ以外のアカウントには無い。
    pub hd: Option<String>,
    /// `profile` scope で載る表示名。Google 側で未設定なら返らない。
    pub name: Option<String>,
    /// `profile` scope で載るプロフィール画像の URL。Google 側で未設定なら返らない。
    pub picture: Option<String>,
}

impl GoogleIdToken {
    /// メールアドレスを Google が持ち主として保証しているか。Gmail か、確認済みの Google Workspace の
    /// メールだけ。それ以外は、Google アカウントを作った後でメールの持ち主が変わっていても
    /// `email_verified` が true のまま残る (Google の「Verify the Google ID token on your server side」)。
    pub fn email_is_trusted(&self) -> bool {
        let Some(email) = self.email.as_deref() else {
            return false;
        };
        self.email_verified
            && (email.to_ascii_lowercase().ends_with("@gmail.com")
                || self.hd.as_deref().is_some_and(|hd| !hd.trim().is_empty()))
    }
}

/// CSRF (login-CSRF) 対策の `state` パラメータを生成する。
pub fn generate_state() -> String {
    random_url_safe(32)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generated_state_is_url_safe_and_not_repeated() {
        let state = generate_state();
        assert!(
            state
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_'),
            "{state}"
        );
        assert_ne!(state, generate_state());
    }

    #[test]
    fn disabled_client_has_no_authorization_url() {
        let client = GoogleLoginClient::disabled();
        assert!(!client.enabled());
        assert!(client.authorization_url("state", "challenge").is_none());
    }

    #[test]
    fn from_env_requires_client_id_and_secret() {
        // 環境変数はテスト間で共有されるプロセス状態のため、他のテストと競合しないよう
        // このテスト内で明示的に unset した状態から確認する。
        // (`#[serial]` 等は導入していないため、Cargo.toml に追加のテスト専用依存を増やさず
        // 素朴に一時的な env 操作で済ませる。)
        unsafe {
            std::env::remove_var(CLIENT_ID_ENV);
            std::env::remove_var(CLIENT_SECRET_ENV);
        }
        assert!(!GoogleLoginClient::from_env("http://localhost:5173").enabled());
    }

    fn claims(email: &str, email_verified: bool, hd: Option<&str>) -> GoogleIdToken {
        GoogleIdToken {
            sub: "sub-1".to_string(),
            email: Some(email.to_string()),
            email_verified,
            hd: hd.map(str::to_string),
            name: None,
            picture: None,
        }
    }

    #[test]
    fn only_gmail_and_workspace_emails_are_trusted() {
        assert!(claims("user@gmail.com", true, None).email_is_trusted());
        assert!(claims("User@GMail.com", true, None).email_is_trusted());
        assert!(claims("user@example.com", true, Some("example.com")).email_is_trusted());
        // Gmail でも Workspace でもないメールで作った Google アカウント。
        assert!(!claims("user@example.com", true, None).email_is_trusted());
        assert!(!claims("user@gmail.com", false, None).email_is_trusted());
        assert!(!claims("user@example.com", false, Some("example.com")).email_is_trusted());
        assert!(!claims("user@example.com", true, Some("")).email_is_trusted());
        assert!(!claims("user@example.com", true, Some(" ")).email_is_trusted());
        // 末尾が似ているだけのドメイン。
        assert!(!claims("user@notgmail.com", true, None).email_is_trusted());
    }
}
