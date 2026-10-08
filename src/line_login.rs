//! LINE ログイン (v2.1、Authorization Code + PKCE + nonce) のクライアント。
//!
//! ID トークンは LINE の検証エンドポイントに nonce 付きで送って検証し、その応答 (ペイロード) を使う。
//! 署名を手元で確かめる JWT のライブラリを増やさないため。
//!
//! LINE の開発ガイドラインは、退会時に連携を取り消すことを必須にしている。取り消しには利用者の
//! アクセストークンが要るので、ログインのたびにトークンを暗号化して保存しておき
//! ([`LineLoginClient::seal_token`])、アカウントを物理削除するときに [`LineLoginClient::revoke`] で取り消す。
//! ウェブのログインはリフレッシュトークンを、アプリのログイン (LINE SDK) はアクセストークンを保存する。

use serde::Deserialize;

use crate::config::non_empty_env;
use crate::external_login::{ProviderError, RevokeError};
use crate::oauth_identity::RevocationTokens;
use crate::token_cipher::TokenCipher;
use crate::upstream::{http_client, parse_success, success_body};

/// コールバックの相対パス (`config.server.public_url` と結合して redirect_uri を作る)。
pub const CALLBACK_PATH: &str = "/api/v1/auth/line/callback";

/// LINE ログインチャネルのチャネルID・チャネルシークレットの環境変数。機微情報のため config.toml には置かない。
pub const CHANNEL_ID_ENV: &str = "LINE_LOGIN_CHANNEL_ID";
pub const CHANNEL_SECRET_ENV: &str = "LINE_LOGIN_CHANNEL_SECRET";

/// ログで呼び出し元を見分けるための名前 (`upstream::success_body`)。
const PROVIDER: &str = "line";

/// 保存するトークンの暗号の鍵を、チャネルシークレットから導くときの用途名。改名前の名前のままにする。
/// 変えると、保存済みのトークンを開けなくなる。
const TOKEN_CIPHER_PURPOSE: &str = "bp-tracker/line-refresh-token/v1";

/// LINE のエンドポイント。統合テストではモックサーバーに差し替える。
#[derive(Debug, Clone)]
pub struct Endpoints {
    pub authorize: String,
    pub token: String,
    pub verify: String,
    pub profile: String,
    pub channel_token: String,
    pub deauthorize: String,
}

impl Default for Endpoints {
    fn default() -> Self {
        Self {
            authorize: "https://access.line.me/oauth2/v2.1/authorize".to_string(),
            token: "https://api.line.me/oauth2/v2.1/token".to_string(),
            verify: "https://api.line.me/oauth2/v2.1/verify".to_string(),
            profile: "https://api.line.me/v2/profile".to_string(),
            // ステートレスチャネルアクセストークン (15分、発行数の制限なし) の発行。
            channel_token: "https://api.line.me/oauth2/v3/token".to_string(),
            deauthorize: "https://api.line.me/user/v1/deauthorize".to_string(),
        }
    }
}

struct Channel {
    id: String,
    secret: String,
}

pub struct LineLoginClient {
    channel: Option<Channel>,
    /// `None` なら ([`Self::disabled`]) ログインは無効。連携の取り消しはチャネルだけでできる。
    redirect_uri: Option<String>,
    cipher: Option<TokenCipher>,
    endpoints: Endpoints,
    http: reqwest::Client,
}

// チャネルシークレットをログに出さない (`GoogleLoginClient` と同じ理由)。
impl std::fmt::Debug for LineLoginClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LineLoginClient")
            .field("enabled", &self.enabled())
            .finish()
    }
}

impl LineLoginClient {
    /// `LINE_LOGIN_CHANNEL_ID` / `LINE_LOGIN_CHANNEL_SECRET` (空文字列は未設定扱い) と
    /// `config.server.public_url` から作る。
    pub fn from_env(base_url: &str) -> Self {
        let channel = match (
            non_empty_env(CHANNEL_ID_ENV),
            non_empty_env(CHANNEL_SECRET_ENV),
        ) {
            (Some(id), Some(secret)) => Some(Channel { id, secret }),
            _ => None,
        };
        let base_url = base_url.trim_end_matches('/');
        let redirect_uri = Some(format!("{base_url}{CALLBACK_PATH}"));
        Self::new(channel, redirect_uri, Endpoints::default())
    }

    /// 常に無効なクライアント。統合テストで、環境変数によらず無効時の挙動を確かめるときに使う。
    pub fn disabled() -> Self {
        Self::new(None, None, Endpoints::default())
    }

    /// 統合テストから、モックサーバーのエンドポイントで有効にしたクライアントを作る。
    /// `GoogleLoginClient::for_test` と同じ理由で `#[cfg(test)]` にはしない。
    pub fn for_test(
        channel_id: impl Into<String>,
        channel_secret: impl Into<String>,
        redirect_uri: impl Into<String>,
        endpoints: Endpoints,
    ) -> Self {
        Self::new(
            Some(Channel {
                id: channel_id.into(),
                secret: channel_secret.into(),
            }),
            Some(redirect_uri.into()),
            endpoints,
        )
    }

    fn new(channel: Option<Channel>, redirect_uri: Option<String>, endpoints: Endpoints) -> Self {
        let cipher = channel
            .as_ref()
            .map(|channel| TokenCipher::derive(TOKEN_CIPHER_PURPOSE, &channel.secret));
        Self {
            channel,
            redirect_uri,
            cipher,
            endpoints,
            http: http_client(),
        }
    }

    /// ログインを受け付けるか。
    pub fn enabled(&self) -> bool {
        self.channel.is_some() && self.redirect_uri.is_some()
    }

    /// LINE の認可エンドポイントへの URL。`enabled() == false` なら `None`。
    pub fn authorization_url(
        &self,
        state: &str,
        code_challenge: &str,
        nonce: &str,
    ) -> Option<String> {
        let channel = self.channel.as_ref()?;
        let redirect_uri = self.redirect_uri.as_deref()?;
        let url = reqwest::Url::parse_with_params(
            &self.endpoints.authorize,
            &[
                ("response_type", "code"),
                ("client_id", channel.id.as_str()),
                ("redirect_uri", redirect_uri),
                ("state", state),
                ("scope", "openid profile email"),
                ("nonce", nonce),
                ("code_challenge", code_challenge),
                ("code_challenge_method", "S256"),
            ],
        )
        .ok()?;
        Some(url.to_string())
    }

    /// 認可コードをトークンに換える。
    pub async fn exchange_code(
        &self,
        code: &str,
        code_verifier: &str,
    ) -> Result<LineTokens, ProviderError> {
        let channel = self.channel.as_ref().ok_or(ProviderError::Disabled)?;
        let redirect_uri = self
            .redirect_uri
            .as_deref()
            .ok_or(ProviderError::Disabled)?;
        let response = self
            .http
            .post(&self.endpoints.token)
            .form(&[
                ("grant_type", "authorization_code"),
                ("code", code),
                ("redirect_uri", redirect_uri),
                ("client_id", channel.id.as_str()),
                ("client_secret", channel.secret.as_str()),
                ("code_verifier", code_verifier),
            ])
            .send()
            .await?;
        parse_success(response, PROVIDER, "token")
            .await
            .map_err(ProviderError::from)
    }

    /// ID トークンを LINE の検証エンドポイントで確かめ、そのペイロードを返す。署名・発行元・
    /// 宛先 (チャネルID)・期限・nonce は LINE が確かめ、合わなければ 400 を返す。
    pub async fn verify_id_token(
        &self,
        id_token: &str,
        nonce: &str,
    ) -> Result<LineIdToken, ProviderError> {
        let channel = self.channel.as_ref().ok_or(ProviderError::Disabled)?;
        let response = self
            .http
            .post(&self.endpoints.verify)
            .form(&[
                ("id_token", id_token),
                ("client_id", channel.id.as_str()),
                ("nonce", nonce),
            ])
            .send()
            .await?;
        parse_success(response, PROVIDER, "verify")
            .await
            .map_err(ProviderError::from)
    }

    /// アクセストークンの持ち主の LINE のユーザーID。アプリから受け取ったアクセストークンが、
    /// 一緒に受け取った ID トークンの本人のものかを確かめるのに使う。
    pub async fn access_token_user_id(&self, access_token: &str) -> Result<String, ProviderError> {
        let response = self
            .http
            .get(&self.endpoints.profile)
            .bearer_auth(access_token)
            .send()
            .await?;
        let profile: Profile = parse_success(response, PROVIDER, "profile").await?;
        Ok(profile.user_id)
    }

    /// 連携の取り消しに使うトークン (リフレッシュトークン・アクセストークン) を、DB に置ける形に
    /// 暗号化する。チャネルが無ければ `None`。
    pub fn seal_token(&self, token: &str) -> Option<String> {
        Some(self.cipher.as_ref()?.seal(token))
    }

    /// 保存しておいたトークンで、利用者が LINE で認可した権限を取り消す (連携の解除)。
    ///
    /// リフレッシュトークン (最後にウェブで LINE にログインしてから90日で切れ、更新しても延びない)
    /// があれば、アクセストークンを取り直して使う。取り消せなければ、アプリのログインで保存した
    /// アクセストークン (発行から30日) をそのまま使う。どちらも切れていれば取り消せない
    /// ([`RevokeError::Upstream`])。
    pub async fn revoke(&self, tokens: &RevocationTokens) -> Result<(), RevokeError> {
        if let Some(sealed) = &tokens.refresh_token {
            match self.revoke_with_refresh_token(sealed).await {
                Ok(()) => return Ok(()),
                Err(err) if tokens.access_token.is_some() => tracing::warn!(
                    error = %crate::error_chain_line(&err),
                    "LINE のリフレッシュトークンでは連携を取り消せませんでした。アクセストークンで試します"
                ),
                Err(err) => return Err(err),
            }
        }
        let sealed = tokens.access_token.as_ref().ok_or(RevokeError::NoToken)?;
        let access_token = self.open(sealed)?;
        self.deauthorize(&access_token).await
    }

    async fn revoke_with_refresh_token(&self, sealed: &str) -> Result<(), RevokeError> {
        let channel = self.channel.as_ref().ok_or(RevokeError::Disabled)?;
        let refresh_token = self.open(sealed)?;
        let response = self
            .http
            .post(&self.endpoints.token)
            .form(&[
                ("grant_type", "refresh_token"),
                ("refresh_token", refresh_token.as_str()),
                ("client_id", channel.id.as_str()),
                ("client_secret", channel.secret.as_str()),
            ])
            .send()
            .await?;
        let user_token: AccessToken = parse_success(response, PROVIDER, "refresh").await?;
        self.deauthorize(&user_token.access_token).await
    }

    /// 利用者のアクセストークンで、連携を取り消す。
    async fn deauthorize(&self, user_access_token: &str) -> Result<(), RevokeError> {
        let channel = self.channel.as_ref().ok_or(RevokeError::Disabled)?;
        let response = self
            .http
            .post(&self.endpoints.channel_token)
            .form(&[
                ("grant_type", "client_credentials"),
                ("client_id", channel.id.as_str()),
                ("client_secret", channel.secret.as_str()),
            ])
            .send()
            .await?;
        let channel_token: AccessToken = parse_success(response, PROVIDER, "channel_token").await?;

        let response = self
            .http
            .post(&self.endpoints.deauthorize)
            .bearer_auth(&channel_token.access_token)
            .json(&serde_json::json!({ "userAccessToken": user_access_token }))
            .send()
            .await?;
        success_body(response, PROVIDER, "deauthorize").await?;
        Ok(())
    }

    fn open(&self, sealed: &str) -> Result<String, RevokeError> {
        if self.channel.is_none() {
            return Err(RevokeError::Disabled);
        }
        self.cipher
            .as_ref()
            .and_then(|cipher| cipher.open(sealed))
            .ok_or(RevokeError::Undecryptable)
    }
}

/// 認可コードと引き換えに受け取るトークン。アクセストークンは使わないので受け取らない。
#[derive(Debug, Deserialize)]
pub struct LineTokens {
    pub id_token: String,
    pub refresh_token: String,
}

#[derive(Debug, Deserialize)]
struct AccessToken {
    access_token: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Profile {
    user_id: String,
}

/// 検証済みの ID トークンのペイロード。使わない項目は受け取らない。
#[derive(Debug, Deserialize)]
pub struct LineIdToken {
    /// LINE のユーザーID。プロバイダー単位で変わらない。
    pub sub: String,
    /// 同意画面でメールアドレスの提供をオフにされると載らない。
    pub email: Option<String>,
    pub name: Option<String>,
    pub picture: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disabled_client_has_no_authorization_url() {
        let client = LineLoginClient::disabled();
        assert!(!client.enabled());
        assert!(
            client
                .authorization_url("state", "challenge", "nonce")
                .is_none()
        );
        assert!(client.seal_token("token").is_none());
    }

    #[test]
    fn authorization_url_asks_for_the_email_with_pkce_and_nonce() {
        let client = LineLoginClient::for_test(
            "1234",
            "secret",
            "https://bp.example.com/api/v1/auth/line/callback",
            Endpoints::default(),
        );
        let url = client
            .authorization_url("state1", "challenge1", "nonce1")
            .expect("有効なクライアントは URL を作れるはず");
        let url = reqwest::Url::parse(&url).expect("URL として読めるはず");
        let query: std::collections::HashMap<_, _> = url.query_pairs().into_owned().collect();
        assert_eq!(query["client_id"], "1234");
        assert_eq!(
            query["redirect_uri"],
            "https://bp.example.com/api/v1/auth/line/callback"
        );
        assert_eq!(query["scope"], "openid profile email");
        assert_eq!(query["state"], "state1");
        assert_eq!(query["nonce"], "nonce1");
        assert_eq!(query["code_challenge"], "challenge1");
        assert_eq!(query["code_challenge_method"], "S256");
    }

    #[test]
    fn redirect_uri_is_needed_to_log_in_but_not_to_seal_tokens() {
        let client = LineLoginClient::new(
            Some(Channel {
                id: "1234".to_string(),
                secret: "secret".to_string(),
            }),
            None,
            Endpoints::default(),
        );
        assert!(!client.enabled());
        assert!(client.seal_token("token").is_some());
    }
}
