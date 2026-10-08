//! Sign in with Apple のクライアント (docs/authentication.md)。
//!
//! ウェブ (サービス ID) とアプリ (Bundle ID) の2つの `client_id` を持つ。どちらも認可コードを
//! Apple のトークンエンドポイントで引き換え、その応答の ID トークンを使う。Apple から直接 TLS で
//! 受け取った ID トークンなので署名は確かめず (OpenID Connect Core 3.1.3.7)、発行元・宛先・期限・
//! 発行時刻・`nonce` を確かめる。
//!
//! 引き換えと取り消しに要る `client_secret` は、Apple の秘密鍵で署名した JWT (ES256)。
//!
//! App Store 審査ガイドライン 5.1.1(v) により、退会・連携の解除ではトークンを取り消す。そのために
//! リフレッシュトークンを暗号化して保存する ([`AppleLoginClient::seal_refresh_token`])。

use p256::ecdsa::SigningKey;
use p256::pkcs8::DecodePrivateKey as _;
use serde::{Deserialize, Deserializer, Serialize};

use crate::config::non_empty_env;
use crate::external_login::{ProviderError, RevokeError};
use crate::token_cipher::TokenCipher;
use crate::upstream::{http_client, parse_success, success_body};

/// コールバックの相対パス (`config.server.public_url` と結合して redirect_uri を作る)。
pub const CALLBACK_PATH: &str = "/api/v1/auth/apple/callback";

/// 機微情報のため config.toml には置かない。
pub const TEAM_ID_ENV: &str = "APPLE_TEAM_ID";
pub const KEY_ID_ENV: &str = "APPLE_KEY_ID";
/// Apple Developer で作った秘密鍵 (.p8、PKCS#8 の PEM) の中身。改行は `\n` と書いてもよい。
pub const PRIVATE_KEY_ENV: &str = "APPLE_PRIVATE_KEY";
/// ウェブの `client_id` (サービス ID)。
pub const SERVICE_ID_ENV: &str = "APPLE_SERVICE_ID";
/// アプリの `client_id` (Bundle ID)。
pub const APP_BUNDLE_ID_ENV: &str = "APPLE_APP_BUNDLE_ID";

/// ログで呼び出し元を見分けるための名前 (`upstream::success_body`)。
const PROVIDER: &str = "apple";

/// ID トークンの発行元 (`iss`)。`client_secret` の宛先 (`aud`) も同じ値。
const ISSUER: &str = "https://appleid.apple.com";

/// 保存するトークンの暗号の鍵を、秘密鍵から導くときの用途名。改名前の名前のままにする。
/// 変えると、保存済みのトークンを開けなくなる。
const TOKEN_CIPHER_PURPOSE: &str = "bp-tracker/apple-refresh-token/v1";

/// ADR: `client_secret` の有効期間 (秒)。Apple は最長6か月を認めるが、呼ぶたびに作るので短くする。
const CLIENT_SECRET_TTL_SECS: i64 = 5 * 60;

/// ADR: ID トークンの発行時刻 (`iat`) として受け入れる古さの上限 (秒)。引き換えの直後に確かめるので
/// ふつうは数秒だが、時計のずれを見込む。未来側も同じ幅だけ許す。
const ID_TOKEN_MAX_AGE_SECS: i64 = 10 * 60;

/// Apple のエンドポイント。統合テストではモックサーバーに差し替える。
#[derive(Debug, Clone)]
pub struct Endpoints {
    pub authorize: String,
    pub token: String,
    pub revoke: String,
}

impl Default for Endpoints {
    fn default() -> Self {
        Self {
            authorize: "https://appleid.apple.com/auth/authorize".to_string(),
            token: "https://appleid.apple.com/auth/token".to_string(),
            revoke: "https://appleid.apple.com/auth/revoke".to_string(),
        }
    }
}

/// 認可コードを発行した側。引き換え・取り消しでは、それぞれ発行した側の `client_id` を使う。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Client {
    Web,
    App,
}

/// 秘密鍵と、それで `client_secret` を作るための ID。
struct Signer {
    team_id: String,
    key_id: String,
    key: SigningKey,
}

/// 各 ID の設定。統合テストから組み立てる。
pub struct Settings {
    pub team_id: String,
    pub key_id: String,
    /// PKCS#8 の PEM。
    pub private_key_pem: String,
    pub service_id: Option<String>,
    pub app_bundle_id: Option<String>,
}

pub struct AppleLoginClient {
    signer: Option<Signer>,
    service_id: Option<String>,
    app_bundle_id: Option<String>,
    /// `None` なら ([`web_redirect_uri`] の条件を満たさない・[`Self::disabled`]) ウェブのログインは無効。
    redirect_uri: Option<String>,
    cipher: Option<TokenCipher>,
    endpoints: Endpoints,
    http: reqwest::Client,
}

// 秘密鍵をログに出さない (`LineLoginClient` と同じ理由)。
impl std::fmt::Debug for AppleLoginClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AppleLoginClient")
            .field("web_enabled", &self.enabled())
            .field("app_enabled", &self.app_enabled())
            .finish()
    }
}

/// ウェブのログインの戻り先。Apple は https の戻り先しか認めず、戻りの `form_post` に
/// `state` の Cookie を届けるには `SameSite=None` (Secure が必須) が要る (→ `oauth_cookie`)。
/// どちらかを満たさない構成ではウェブのログインを無効にする (有効にしても必ず失敗するため)。
fn web_redirect_uri(base_url: &str, secure_cookie: bool) -> Option<String> {
    let base_url = base_url.trim_end_matches('/');
    (secure_cookie && base_url.starts_with("https://"))
        .then(|| format!("{base_url}{CALLBACK_PATH}"))
}

impl AppleLoginClient {
    /// 環境変数 (空文字列は未設定扱い) と `config.server.public_url`・`session.secure_cookie` から作る。
    /// 秘密鍵が読めなければ、ログに残して無効にする。
    pub fn from_env(base_url: &str, secure_cookie: bool) -> Self {
        let settings = match (
            non_empty_env(TEAM_ID_ENV),
            non_empty_env(KEY_ID_ENV),
            non_empty_env(PRIVATE_KEY_ENV),
        ) {
            (Some(team_id), Some(key_id), Some(private_key_pem)) => Some(Settings {
                team_id,
                key_id,
                private_key_pem: private_key_pem.replace("\\n", "\n"),
                service_id: non_empty_env(SERVICE_ID_ENV),
                app_bundle_id: non_empty_env(APP_BUNDLE_ID_ENV),
            }),
            _ => None,
        };
        Self::new(
            settings,
            web_redirect_uri(base_url, secure_cookie),
            Endpoints::default(),
        )
    }

    /// 常に無効なクライアント。統合テストで、環境変数によらず無効時の挙動を確かめるときに使う。
    pub fn disabled() -> Self {
        Self::new(None, None, Endpoints::default())
    }

    /// 統合テストから、モックサーバーのエンドポイントで有効にしたクライアントを作る。
    /// `LineLoginClient::for_test` と同じ理由で `#[cfg(test)]` にはしない。
    pub fn for_test(
        settings: Settings,
        redirect_uri: impl Into<String>,
        endpoints: Endpoints,
    ) -> Self {
        Self::new(Some(settings), Some(redirect_uri.into()), endpoints)
    }

    fn new(settings: Option<Settings>, redirect_uri: Option<String>, endpoints: Endpoints) -> Self {
        let (signer, cipher, service_id, app_bundle_id) = match settings {
            Some(settings) => match SigningKey::from_pkcs8_pem(&settings.private_key_pem) {
                Ok(key) => (
                    Some(Signer {
                        team_id: settings.team_id,
                        key_id: settings.key_id,
                        key,
                    }),
                    Some(TokenCipher::derive(
                        TOKEN_CIPHER_PURPOSE,
                        &settings.private_key_pem,
                    )),
                    settings.service_id,
                    settings.app_bundle_id,
                ),
                Err(err) => {
                    tracing::error!(error = %crate::error_chain_line(&err), "{PRIVATE_KEY_ENV} を秘密鍵として読めないため、Sign in with Apple は無効です");
                    (None, None, None, None)
                }
            },
            None => (None, None, None, None),
        };
        Self {
            signer,
            service_id,
            app_bundle_id,
            redirect_uri,
            cipher,
            endpoints,
            http: http_client(),
        }
    }

    /// ウェブのログインを受け付けるか。
    pub fn enabled(&self) -> bool {
        self.signer.is_some() && self.service_id.is_some() && self.redirect_uri.is_some()
    }

    /// アプリのログインを受け付けるか。
    pub fn app_enabled(&self) -> bool {
        self.signer.is_some() && self.app_bundle_id.is_some()
    }

    fn client_id(&self, client: Client) -> Option<&str> {
        match client {
            Client::Web => self.service_id.as_deref(),
            Client::App => self.app_bundle_id.as_deref(),
        }
    }

    /// Apple の認可エンドポイントへの URL。`enabled() == false` なら `None`。
    ///
    /// 名前・メールアドレスを求めるので、戻りは `form_post` (Apple の求め)。
    pub fn authorization_url(&self, state: &str, nonce: &str) -> Option<String> {
        if !self.enabled() {
            return None;
        }
        let client_id = self.service_id.as_deref()?;
        let redirect_uri = self.redirect_uri.as_deref()?;
        let url = reqwest::Url::parse_with_params(
            &self.endpoints.authorize,
            &[
                ("response_type", "code"),
                ("response_mode", "form_post"),
                ("client_id", client_id),
                ("redirect_uri", redirect_uri),
                ("scope", "name email"),
                ("state", state),
                ("nonce", nonce),
            ],
        )
        .ok()?;
        Some(url.to_string())
    }

    /// 認可コードを引き換え、ID トークンを確かめる。
    pub async fn exchange_code(
        &self,
        client: Client,
        code: &str,
        nonce: &str,
    ) -> Result<AppleTokens, ProviderError> {
        let client_id = self.client_id(client).ok_or(ProviderError::Disabled)?;
        let client_secret = self.client_secret(client_id)?;
        let mut form = vec![
            ("grant_type", "authorization_code"),
            ("code", code),
            ("client_id", client_id),
            ("client_secret", client_secret.as_str()),
        ];
        // ウェブの認可コードは、認可を求めたときの redirect_uri と一緒に引き換える。
        if client == Client::Web {
            form.push((
                "redirect_uri",
                self.redirect_uri
                    .as_deref()
                    .ok_or(ProviderError::Disabled)?,
            ));
        }
        let response = self
            .http
            .post(&self.endpoints.token)
            .form(&form)
            .send()
            .await?;
        let tokens: TokenResponse = parse_success(response, PROVIDER, "token").await?;
        let id_token = verify_id_token(&tokens.id_token, client_id, nonce, now_secs())
            .ok_or(ProviderError::IdTokenMismatch)?;
        Ok(AppleTokens {
            id_token,
            refresh_token: tokens.refresh_token,
        })
    }

    /// リフレッシュトークンを、発行した側と一緒に DB に置ける形に暗号化する。設定が無ければ `None`。
    pub fn seal_refresh_token(&self, client: Client, refresh_token: &str) -> Option<String> {
        let stored = StoredRefreshToken {
            client,
            token: refresh_token.to_string(),
        };
        let json = serde_json::to_string(&stored).ok()?;
        Some(self.cipher.as_ref()?.seal(&json))
    }

    /// 保存しておいたリフレッシュトークンを取り消す (退会・連携の解除)。
    pub async fn revoke(&self, sealed_refresh_token: &str) -> Result<(), RevokeError> {
        let cipher = self.cipher.as_ref().ok_or(RevokeError::Disabled)?;
        let stored: StoredRefreshToken = cipher
            .open(sealed_refresh_token)
            .and_then(|json| serde_json::from_str(&json).ok())
            .ok_or(RevokeError::Undecryptable)?;
        let client_id = self.client_id(stored.client).ok_or(RevokeError::Disabled)?;
        let client_secret = self.client_secret(client_id)?;
        let response = self
            .http
            .post(&self.endpoints.revoke)
            .form(&[
                ("client_id", client_id),
                ("client_secret", client_secret.as_str()),
                ("token", stored.token.as_str()),
                ("token_type_hint", "refresh_token"),
            ])
            .send()
            .await?;
        success_body(response, PROVIDER, "revoke").await?;
        Ok(())
    }

    /// `client_id` 宛ての `client_secret` (ES256 で署名した JWT)。
    fn client_secret(&self, client_id: &str) -> Result<String, ProviderError> {
        let signer = self.signer.as_ref().ok_or(ProviderError::Disabled)?;
        let now = now_secs();
        let header = serde_json::json!({ "alg": "ES256", "kid": signer.key_id });
        let claims = serde_json::json!({
            "iss": signer.team_id,
            "iat": now,
            "exp": now + CLIENT_SECRET_TTL_SECS,
            "aud": ISSUER,
            "sub": client_id,
        });
        Ok(crate::jwt::sign_es256(&signer.key, &header, &claims))
    }
}

fn now_secs() -> i64 {
    jiff::Timestamp::now().as_second()
}

#[derive(Debug, Deserialize)]
struct TokenResponse {
    id_token: String,
    /// 応答に無いことがある。そのときは保存済みの値を残す (docs/authentication.md)。
    refresh_token: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
struct StoredRefreshToken {
    client: Client,
    token: String,
}

/// 認可コードを引き換えて得たもの。
#[derive(Debug)]
pub struct AppleTokens {
    pub id_token: AppleIdToken,
    pub refresh_token: Option<String>,
}

/// 確かめた ID トークンのうち、使う項目。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppleIdToken {
    /// Apple のユーザー ID。同じチームのアプリ・サービス ID の間で同じ値になる。
    pub sub: String,
    pub email: Option<String>,
    pub email_verified: bool,
}

#[derive(Debug, Deserialize)]
struct IdTokenClaims {
    iss: String,
    aud: String,
    exp: i64,
    iat: i64,
    sub: String,
    nonce: Option<String>,
    email: Option<String>,
    #[serde(default, deserialize_with = "bool_or_string")]
    email_verified: bool,
}

/// Apple は真偽値を `true` と `"true"` のどちらでも返しうる。
fn bool_or_string<'de, D: Deserializer<'de>>(deserializer: D) -> Result<bool, D::Error> {
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Value {
        Bool(bool),
        String(String),
    }
    Ok(match Value::deserialize(deserializer)? {
        Value::Bool(value) => value,
        Value::String(value) => value == "true",
    })
}

/// ID トークン (JWT) のペイロードを読み、署名以外を確かめる。合わなければ `None`。
fn verify_id_token(id_token: &str, client_id: &str, nonce: &str, now: i64) -> Option<AppleIdToken> {
    let claims: IdTokenClaims = crate::jwt::unverified_payload(id_token)?;
    let valid = claims.iss == ISSUER
        && claims.aud == client_id
        && claims.exp > now
        && (now - claims.iat).abs() <= ID_TOKEN_MAX_AGE_SECS
        && claims.nonce.as_deref() == Some(nonce);
    valid.then_some(AppleIdToken {
        sub: claims.sub,
        email: claims.email,
        email_verified: claims.email_verified,
    })
}

/// 最初の承認のときにだけ渡る名前を、表示名にする。姓・名のどちらかでもあれば、その並びで返す。
///
/// 日本語などの名前は「姓 名」、英字だけの名前は「名 姓」の順に並べる。
pub fn display_name(given_name: Option<&str>, family_name: Option<&str>) -> Option<String> {
    let given = given_name.map(str::trim).filter(|name| !name.is_empty());
    let family = family_name.map(str::trim).filter(|name| !name.is_empty());
    match (given, family) {
        (Some(given), Some(family)) => {
            if given.is_ascii() && family.is_ascii() {
                Some(format!("{given} {family}"))
            } else {
                Some(format!("{family} {given}"))
            }
        }
        (Some(name), None) | (None, Some(name)) => Some(name.to_string()),
        (None, None) => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::TEST_PRIVATE_KEY;
    use base64::Engine as _;
    use base64::engine::general_purpose::URL_SAFE_NO_PAD;

    fn id_token(claims: serde_json::Value) -> String {
        format!(
            "{}.{}.signature",
            URL_SAFE_NO_PAD.encode(r#"{"alg":"RS256"}"#),
            URL_SAFE_NO_PAD.encode(claims.to_string())
        )
    }

    fn claims(now: i64) -> serde_json::Value {
        serde_json::json!({
            "iss": ISSUER,
            "aud": "com.example.web",
            "exp": now + 600,
            "iat": now,
            "sub": "000123.abc",
            "nonce": "nonce1",
            "email": "user@privaterelay.appleid.com",
            "email_verified": "true",
        })
    }

    #[test]
    fn web_login_needs_https_and_secure_cookie() {
        assert_eq!(
            web_redirect_uri("https://bp.example.com/", true).as_deref(),
            Some("https://bp.example.com/api/v1/auth/apple/callback")
        );
        assert_eq!(web_redirect_uri("https://bp.example.com", false), None);
        assert_eq!(web_redirect_uri("http://localhost:3010", true), None);
        assert_eq!(web_redirect_uri("", true), None);
    }

    #[test]
    fn verify_id_token_accepts_a_matching_token() {
        let now = 1_800_000_000;
        let token = verify_id_token(&id_token(claims(now)), "com.example.web", "nonce1", now)
            .expect("合っている ID トークンは通るはず");
        assert_eq!(token.sub, "000123.abc");
        assert_eq!(
            token.email.as_deref(),
            Some("user@privaterelay.appleid.com")
        );
        assert!(token.email_verified);
    }

    #[test]
    fn verify_id_token_rejects_each_mismatch() {
        let now = 1_800_000_000;
        let cases = [
            ("iss", serde_json::json!("https://evil.example")),
            ("aud", serde_json::json!("com.example.other")),
            ("exp", serde_json::json!(now)),
            ("iat", serde_json::json!(now - ID_TOKEN_MAX_AGE_SECS - 1)),
            ("iat", serde_json::json!(now + ID_TOKEN_MAX_AGE_SECS + 1)),
            ("nonce", serde_json::json!("other")),
            ("nonce", serde_json::Value::Null),
        ];
        for (key, value) in cases {
            let mut claims = claims(now);
            claims[key] = value.clone();
            assert!(
                verify_id_token(&id_token(claims), "com.example.web", "nonce1", now).is_none(),
                "{key} = {value}"
            );
        }
        assert!(verify_id_token("not-a-jwt", "com.example.web", "nonce1", now).is_none());
    }

    #[test]
    fn email_verified_accepts_a_bool_or_a_string() {
        let now = 1_800_000_000;
        for (value, expected) in [
            (serde_json::json!(true), true),
            (serde_json::json!("true"), true),
            (serde_json::json!(false), false),
            (serde_json::json!("false"), false),
        ] {
            let mut claims = claims(now);
            claims["email_verified"] = value;
            let token = verify_id_token(&id_token(claims), "com.example.web", "nonce1", now)
                .expect("ほかは合っているので通るはず");
            assert_eq!(token.email_verified, expected);
        }
    }

    fn client() -> AppleLoginClient {
        AppleLoginClient::for_test(
            Settings {
                team_id: "TEAM123".to_string(),
                key_id: "KEY123".to_string(),
                private_key_pem: TEST_PRIVATE_KEY.to_string(),
                service_id: Some("com.example.web".to_string()),
                app_bundle_id: Some("com.example.app".to_string()),
            },
            "https://bp.example.com/api/v1/auth/apple/callback",
            Endpoints::default(),
        )
    }

    /// `client_secret` は、秘密鍵の公開鍵で確かめられる ES256 の JWT で、Apple の求めるクレームを持つ。
    #[test]
    fn client_secret_is_an_es256_jwt_signed_with_the_private_key() {
        use p256::ecdsa::signature::Verifier as _;

        let client = client();
        let secret = client
            .client_secret("com.example.app")
            .expect("有効なので作れるはず");
        let parts: Vec<&str> = secret.split('.').collect();
        assert_eq!(parts.len(), 3);
        let decode = |part: &str| -> serde_json::Value {
            serde_json::from_slice(&URL_SAFE_NO_PAD.decode(part).expect("base64url")).expect("JSON")
        };
        let header = decode(parts[0]);
        assert_eq!(header["alg"], "ES256");
        assert_eq!(header["kid"], "KEY123");
        let claims = decode(parts[1]);
        assert_eq!(claims["iss"], "TEAM123");
        assert_eq!(claims["aud"], ISSUER);
        assert_eq!(claims["sub"], "com.example.app");
        let iat = claims["iat"].as_i64().expect("iat");
        assert_eq!(claims["exp"].as_i64(), Some(iat + CLIENT_SECRET_TTL_SECS));

        let signature = p256::ecdsa::Signature::from_slice(
            &URL_SAFE_NO_PAD.decode(parts[2]).expect("base64url"),
        )
        .expect("r||s の64バイト");
        let verifying_key = *SigningKey::from_pkcs8_pem(TEST_PRIVATE_KEY)
            .expect("PEM")
            .verifying_key();
        verifying_key
            .verify(format!("{}.{}", parts[0], parts[1]).as_bytes(), &signature)
            .expect("署名が合うはず");
    }

    #[test]
    fn unreadable_private_key_disables_the_client() {
        let client = AppleLoginClient::for_test(
            Settings {
                team_id: "TEAM123".to_string(),
                key_id: "KEY123".to_string(),
                private_key_pem: "not a key".to_string(),
                service_id: Some("com.example.web".to_string()),
                app_bundle_id: Some("com.example.app".to_string()),
            },
            "https://bp.example.com/api/v1/auth/apple/callback",
            Endpoints::default(),
        );
        assert!(!client.enabled());
        assert!(!client.app_enabled());
    }

    #[test]
    fn sealed_refresh_token_keeps_the_client_it_was_issued_to() {
        let client = client();
        let sealed = client
            .seal_refresh_token(Client::App, "refresh-1")
            .expect("有効なので暗号化できるはず");
        assert!(!sealed.contains("refresh-1"));
        let stored: StoredRefreshToken = serde_json::from_str(
            &client
                .cipher
                .as_ref()
                .expect("cipher")
                .open(&sealed)
                .expect("復号できるはず"),
        )
        .expect("JSON");
        assert_eq!(stored.client, Client::App);
        assert_eq!(stored.token, "refresh-1");
    }

    #[test]
    fn authorization_url_asks_for_the_name_and_email_with_form_post() {
        let url = client()
            .authorization_url("state1", "nonce1")
            .expect("有効なので URL を作れるはず");
        let url = reqwest::Url::parse(&url).expect("URL");
        let query: std::collections::HashMap<_, _> = url.query_pairs().into_owned().collect();
        assert_eq!(query["client_id"], "com.example.web");
        assert_eq!(query["response_type"], "code");
        assert_eq!(query["response_mode"], "form_post");
        assert_eq!(query["scope"], "name email");
        assert_eq!(query["state"], "state1");
        assert_eq!(query["nonce"], "nonce1");
        assert_eq!(
            query["redirect_uri"],
            "https://bp.example.com/api/v1/auth/apple/callback"
        );
    }

    #[test]
    fn display_name_orders_by_script() {
        assert_eq!(
            display_name(Some("太郎"), Some("佐藤")).as_deref(),
            Some("佐藤 太郎")
        );
        assert_eq!(
            display_name(Some("Taro"), Some("Sato")).as_deref(),
            Some("Taro Sato")
        );
        assert_eq!(display_name(Some(" 花子 "), None).as_deref(), Some("花子"));
        assert_eq!(display_name(Some(""), Some(" ")), None);
    }
}
