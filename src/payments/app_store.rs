//! App Store Server API の呼び出しと、通知の読み取り (docs/payments.md)。
//! Apple 固有の HTTP 形式・型はこのモジュールに閉じる。
//!
//! 取引は Apple から直接 TLS で取り直したものだけを使うので、JWS の署名 (証明書の連なり) は確かめない。
//! アプリや通知から届く JWS は、取引 ID を知るためだけに読む。

use std::net::IpAddr;

use p256::ecdsa::SigningKey;
use p256::pkcs8::DecodePrivateKey as _;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::config::non_empty_env;
use crate::forwarded::TrustedProxy;
use crate::upstream::{http_client, response_body};

/// いずれも機微情報のため config.toml には置かない。In-App Purchase の鍵 (Sign in with Apple の鍵とは別)。
pub const ISSUER_ID_ENV: &str = "APP_STORE_ISSUER_ID";
pub const KEY_ID_ENV: &str = "APP_STORE_KEY_ID";
/// 秘密鍵 (.p8、PKCS#8 の PEM) の中身。改行は `\n` と書いてもよい。
pub const PRIVATE_KEY_ENV: &str = "APP_STORE_PRIVATE_KEY";

/// 買い足しの商品 (消耗型)。App Store Connect で作った後は変えられない。
pub const PRODUCT_ID: &str = "com.amiiby.bpcarnet.ocr_topup";

/// 取引の種類 (`type`) のうち、消耗型。
const CONSUMABLE: &str = "Consumable";

/// JWT の宛先 (`aud`)。
const AUDIENCE: &str = "appstoreconnect-v1";

/// ADR: JWT の有効期間 (秒)。Apple は最長60分を認めるが、呼ぶたびに作るので短くする。
const TOKEN_TTL_SECS: i64 = 5 * 60;

/// 通知の送り元として Apple が案内している範囲。
const NOTIFICATION_SOURCE: &str = "17.0.0.0/8";

/// ログで呼び出し元を見分けるための名前。
const PROVIDER: &str = "app_store";

/// 取引の環境。App Store Server API と通知の値 (`Production`・`Sandbox`) をそのまま使い、DB にも同じ文字列で残す。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[schema(as = AppStoreEnvironment)]
pub enum Environment {
    Production,
    Sandbox,
}

impl Environment {
    /// `Production`・`Sandbox` 以外 (StoreKit のローカルのテストの `Xcode` など) は `None`。
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "Production" => Some(Self::Production),
            "Sandbox" => Some(Self::Sandbox),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Production => "Production",
            Self::Sandbox => "Sandbox",
        }
    }

    fn other(self) -> Self {
        match self {
            Self::Production => Self::Sandbox,
            Self::Sandbox => Self::Production,
        }
    }
}

/// App Store Server API のエンドポイント。統合テストではモックサーバーに差し替える。
#[derive(Debug, Clone)]
pub struct Endpoints {
    pub production: String,
    pub sandbox: String,
}

impl Default for Endpoints {
    fn default() -> Self {
        Self {
            production: "https://api.storekit.apple.com".to_string(),
            sandbox: "https://api.storekit-sandbox.apple.com".to_string(),
        }
    }
}

impl Endpoints {
    fn base(&self, environment: Environment) -> &str {
        match environment {
            Environment::Production => &self.production,
            Environment::Sandbox => &self.sandbox,
        }
    }
}

/// 鍵と ID。統合テストから組み立てる。
pub struct Settings {
    pub issuer_id: String,
    pub key_id: String,
    /// PKCS#8 の PEM。
    pub private_key_pem: String,
    /// アプリの Bundle ID (`apple_login::APP_BUNDLE_ID_ENV` と同じ値)。
    pub bundle_id: String,
}

struct Credentials {
    issuer_id: String,
    key_id: String,
    key: SigningKey,
    bundle_id: String,
}

#[derive(Debug, thiserror::Error)]
pub enum AppStoreError {
    #[error("app store server api is not configured")]
    Disabled,
    /// 取引として使えない (取引 ID の形が不正・Apple の取引が読めない・問い合わせと合わない)。
    /// 送り直しても変わらないので、足さないと決める。
    #[error("unusable transaction: {0}")]
    Unusable(&'static str),
    /// 通信の失敗・Apple の API の失敗・両方の環境で見つからない等。時間をおけば確かめられうる。
    #[error("app store server api is temporarily unavailable: {0}")]
    Temporary(&'static str),
    #[error("failed to call app store server api")]
    Request(#[source] reqwest::Error),
}

/// `without_url()`: エラーメッセージに URL を含めない (ログに出るため、`StripeError`・`OcrError` と同じ方針)。
impl From<reqwest::Error> for AppStoreError {
    fn from(err: reqwest::Error) -> Self {
        Self::Request(err.without_url())
    }
}

/// Apple から取り直した取引 (`JWSTransactionDecodedPayload` の、使う項目だけ)。
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Transaction {
    pub transaction_id: String,
    pub bundle_id: String,
    pub product_id: String,
    #[serde(rename = "type")]
    pub kind: String,
    pub quantity: Option<i64>,
    /// UNIX 時刻 (ミリ秒)。
    pub purchase_date: i64,
    pub app_account_token: Option<String>,
    pub revocation_date: Option<i64>,
    /// App Store がこの JWS に署名した UNIX 時刻 (ミリ秒)。取り直した順を決めるのに使う。
    pub signed_date: i64,
    pub environment: Environment,
    /// App Store の国 (ISO 3166-1 alpha-3)。
    pub storefront: Option<String>,
}

impl Transaction {
    pub fn is_revoked(&self) -> bool {
        self.revocation_date.is_some()
    }

    /// 買い手が国内か。App Store の国 (`storefront`) で決める。無ければ `None`。
    pub fn buyer_is_domestic(&self) -> Option<bool> {
        self.storefront.as_deref().map(|country| country == "JPN")
    }
}

/// 通知から読んだ、取り直すのに要ること。
#[derive(Debug)]
pub struct NotificationTarget {
    pub notification_type: String,
    pub environment: Environment,
    pub transaction_id: String,
}

/// 通知の読み取りの結果。
#[derive(Debug)]
pub enum Notification {
    /// 購入・返金に関わる通知。取引を取り直して枠を合わせる。
    Transaction(NotificationTarget),
    /// 関係の無い通知 (種類・アプリ・環境が合わない)。
    Ignored,
}

pub struct AppStoreClient {
    credentials: Option<Credentials>,
    endpoints: Endpoints,
    notification_sources: Vec<TrustedProxy>,
    http: reqwest::Client,
}

// 秘密鍵をログに出さない (`AppleLoginClient` と同じ理由)。
impl std::fmt::Debug for AppStoreClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AppStoreClient")
            .field("enabled", &self.enabled())
            .finish()
    }
}

impl AppStoreClient {
    /// 環境変数 (空文字列は未設定扱い) から作る。鍵か Bundle ID が欠けるか、秘密鍵が読めなければ無効。
    pub fn from_env() -> Self {
        let settings = match (
            non_empty_env(ISSUER_ID_ENV),
            non_empty_env(KEY_ID_ENV),
            non_empty_env(PRIVATE_KEY_ENV),
            non_empty_env(crate::apple_login::APP_BUNDLE_ID_ENV),
        ) {
            (Some(issuer_id), Some(key_id), Some(private_key_pem), Some(bundle_id)) => {
                Some(Settings {
                    issuer_id,
                    key_id,
                    private_key_pem: private_key_pem.replace("\\n", "\n"),
                    bundle_id,
                })
            }
            _ => None,
        };
        let sources = vec![
            NOTIFICATION_SOURCE
                .parse()
                .expect("NOTIFICATION_SOURCE should be a valid CIDR"),
        ];
        Self::new(settings, Endpoints::default(), sources)
    }

    /// 常に無効なクライアント。統合テストで、環境変数によらず無効時の挙動を確かめるときに使う。
    pub fn disabled() -> Self {
        Self::new(None, Endpoints::default(), Vec::new())
    }

    /// 統合テストから、モックサーバーのエンドポイントと通知の送り元の範囲で有効にしたクライアントを作る。
    /// `AppleLoginClient::for_test` と同じ理由で `#[cfg(test)]` にはしない。
    pub fn for_test(
        settings: Settings,
        endpoints: Endpoints,
        notification_sources: Vec<TrustedProxy>,
    ) -> Self {
        Self::new(Some(settings), endpoints, notification_sources)
    }

    fn new(
        settings: Option<Settings>,
        endpoints: Endpoints,
        notification_sources: Vec<TrustedProxy>,
    ) -> Self {
        let credentials = settings.and_then(|settings| {
            match SigningKey::from_pkcs8_pem(&settings.private_key_pem) {
                Ok(key) => Some(Credentials {
                    issuer_id: settings.issuer_id,
                    key_id: settings.key_id,
                    key,
                    bundle_id: settings.bundle_id,
                }),
                Err(err) => {
                    tracing::error!(error = %crate::error_chain_line(&err), "{PRIVATE_KEY_ENV} を秘密鍵として読めないため、アプリ内課金は無効です");
                    None
                }
            }
        });
        Self {
            credentials,
            endpoints,
            notification_sources,
            http: http_client(),
        }
    }

    pub fn enabled(&self) -> bool {
        self.credentials.is_some()
    }

    pub fn bundle_id(&self) -> Option<&str> {
        self.credentials
            .as_ref()
            .map(|creds| creds.bundle_id.as_str())
    }

    /// 通知の送り元として受け付けるか。接続元が分からなければ受け付けない。
    pub fn is_notification_source(&self, client_ip: Option<IpAddr>) -> bool {
        client_ip.is_some_and(|ip| {
            self.notification_sources
                .iter()
                .any(|source| source.contains(ip))
        })
    }

    /// アプリが添えた環境で取引を引き、見つからなければもう一方の環境でも引く。
    pub async fn find_transaction(
        &self,
        transaction_id: &str,
        environment: Environment,
    ) -> Result<Transaction, AppStoreError> {
        match self.get_transaction(transaction_id, environment).await? {
            Some(transaction) => Ok(transaction),
            None => self
                .get_transaction(transaction_id, environment.other())
                .await?
                // Apple の説明では、見つからないことがまれに一時的に起きるため、足さないと決めない。
                .ok_or(AppStoreError::Temporary("transaction not found")),
        }
    }

    /// Get Transaction Info。その環境に無ければ `None`。
    pub async fn get_transaction(
        &self,
        transaction_id: &str,
        environment: Environment,
    ) -> Result<Option<Transaction>, AppStoreError> {
        let creds = self.credentials.as_ref().ok_or(AppStoreError::Disabled)?;
        // 取引 ID は数字の列。URL のパスに入れるので、それ以外は Apple に送らない。
        if transaction_id.is_empty()
            || transaction_id.len() > 32
            || !transaction_id.bytes().all(|b| b.is_ascii_digit())
        {
            return Err(AppStoreError::Unusable("malformed transaction id"));
        }
        let url = format!(
            "{}/inApps/v1/transactions/{transaction_id}",
            self.endpoints.base(environment)
        );
        let response = self
            .http
            .get(&url)
            .bearer_auth(self.token(creds))
            .send()
            .await?;
        let (status, body) = response_body(response).await?;
        match status.as_u16() {
            200 => {}
            404 => return Ok(None),
            400 => return Err(AppStoreError::Unusable("rejected by app store")),
            code => {
                // 401 (鍵の設定の誤り)・429・5xx。設定を直すか時間をおけば確かめられるので、足さないと決めない。
                tracing::warn!(
                    status = code,
                    provider = PROVIDER,
                    environment = environment.as_str(),
                    "App Store Server API がエラーを返しました"
                );
                return Err(AppStoreError::Temporary("unexpected status"));
            }
        }
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct TransactionInfoResponse {
            signed_transaction_info: String,
        }
        let parsed: TransactionInfoResponse = serde_json::from_str(&body)
            .map_err(|_| AppStoreError::Temporary("unexpected response"))?;
        // 200 で届いた取引が読めない・合わないのは、送り直しても変わらない。
        let transaction: Transaction =
            crate::jwt::unverified_payload(&parsed.signed_transaction_info)
                .ok_or(AppStoreError::Unusable("unreadable transaction"))?;
        if transaction.transaction_id != transaction_id || transaction.environment != environment {
            return Err(AppStoreError::Unusable("mismatched transaction"));
        }
        Ok(Some(transaction))
    }

    /// 通知の本文 (`responseBodyV2`) を読む。形が不正なら `None`。
    pub fn read_notification(&self, body: &[u8]) -> Option<Notification> {
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct Body {
            signed_payload: String,
        }
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct Payload {
            notification_type: String,
            data: Option<Data>,
        }
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct Data {
            bundle_id: Option<String>,
            environment: Option<String>,
            signed_transaction_info: Option<String>,
        }
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct TransactionId {
            transaction_id: String,
        }

        let body: Body = serde_json::from_slice(body).ok()?;
        let payload: Payload = crate::jwt::unverified_payload(&body.signed_payload)?;
        if !matches!(
            payload.notification_type.as_str(),
            "ONE_TIME_CHARGE" | "REFUND" | "REFUND_REVERSED"
        ) {
            return Some(Notification::Ignored);
        }
        let Some(data) = payload.data else {
            return Some(Notification::Ignored);
        };
        if data.bundle_id.as_deref() != self.bundle_id() {
            return Some(Notification::Ignored);
        }
        let Some(environment) = data.environment.as_deref().and_then(Environment::parse) else {
            return Some(Notification::Ignored);
        };
        let transaction: TransactionId =
            crate::jwt::unverified_payload(data.signed_transaction_info.as_deref()?)?;
        Some(Notification::Transaction(NotificationTarget {
            notification_type: payload.notification_type,
            environment,
            transaction_id: transaction.transaction_id,
        }))
    }

    /// 要求ごとに作る、App Store Server API の JWT (ES256)。
    fn token(&self, creds: &Credentials) -> String {
        let now = jiff::Timestamp::now().as_second();
        let header = serde_json::json!({ "alg": "ES256", "kid": creds.key_id, "typ": "JWT" });
        let claims = serde_json::json!({
            "iss": creds.issuer_id,
            "iat": now,
            "exp": now + TOKEN_TTL_SECS,
            "aud": AUDIENCE,
            "bid": creds.bundle_id,
        });
        crate::jwt::sign_es256(&creds.key, &header, &claims)
    }
}

/// 取引が買い足しの商品として足してよいものか (環境と取り消しは見ない)。
pub fn is_topup_product(transaction: &Transaction, bundle_id: &str) -> bool {
    transaction.bundle_id == bundle_id
        && transaction.kind == CONSUMABLE
        && transaction.product_id == PRODUCT_ID
        && transaction.quantity == Some(1)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::TEST_PRIVATE_KEY;
    use base64::Engine as _;
    use base64::engine::general_purpose::URL_SAFE_NO_PAD;

    fn client() -> AppStoreClient {
        AppStoreClient::for_test(
            Settings {
                issuer_id: "ISSUER".to_string(),
                key_id: "KEY123".to_string(),
                private_key_pem: TEST_PRIVATE_KEY.to_string(),
                bundle_id: "com.example.app".to_string(),
            },
            Endpoints::default(),
            vec!["17.0.0.0/8".parse().expect("CIDR")],
        )
    }

    fn jws(payload: serde_json::Value) -> String {
        format!("e30.{}.c2ln", URL_SAFE_NO_PAD.encode(payload.to_string()))
    }

    #[test]
    fn token_is_an_es256_jwt_for_the_app_store_server_api() {
        use p256::ecdsa::signature::Verifier as _;

        let client = client();
        let token = client.token(client.credentials.as_ref().expect("有効"));
        let parts: Vec<&str> = token.split('.').collect();
        assert_eq!(parts.len(), 3);
        let decode = |part: &str| -> serde_json::Value {
            serde_json::from_slice(&URL_SAFE_NO_PAD.decode(part).expect("base64url")).expect("JSON")
        };
        let header = decode(parts[0]);
        assert_eq!(header["alg"], "ES256");
        assert_eq!(header["kid"], "KEY123");
        assert_eq!(header["typ"], "JWT");
        let claims = decode(parts[1]);
        assert_eq!(claims["iss"], "ISSUER");
        assert_eq!(claims["aud"], AUDIENCE);
        assert_eq!(claims["bid"], "com.example.app");
        let iat = claims["iat"].as_i64().expect("iat");
        assert_eq!(claims["exp"].as_i64(), Some(iat + TOKEN_TTL_SECS));

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
    fn notification_sources_are_limited_to_the_configured_range() {
        let client = client();
        assert!(client.is_notification_source(Some("17.1.2.3".parse().expect("IP"))));
        assert!(!client.is_notification_source(Some("18.1.2.3".parse().expect("IP"))));
        assert!(!client.is_notification_source(None));
    }

    #[test]
    fn reads_the_transaction_id_of_a_purchase_notification() {
        let body = serde_json::json!({
            "signedPayload": jws(serde_json::json!({
                "notificationType": "REFUND",
                "data": {
                    "bundleId": "com.example.app",
                    "environment": "Sandbox",
                    "signedTransactionInfo": jws(serde_json::json!({ "transactionId": "2000000001" })),
                },
            })),
        });
        let Some(Notification::Transaction(target)) =
            client().read_notification(body.to_string().as_bytes())
        else {
            panic!("購入・返金の通知として読めるはず");
        };
        assert_eq!(target.notification_type, "REFUND");
        assert_eq!(target.environment, Environment::Sandbox);
        assert_eq!(target.transaction_id, "2000000001");
    }

    #[test]
    fn ignores_unrelated_notifications() {
        let notification = |notification_type: &str, bundle_id: &str, environment: &str| {
            serde_json::json!({
                "signedPayload": jws(serde_json::json!({
                    "notificationType": notification_type,
                    "data": {
                        "bundleId": bundle_id,
                        "environment": environment,
                        "signedTransactionInfo": jws(serde_json::json!({ "transactionId": "1" })),
                    },
                })),
            })
            .to_string()
        };
        for body in [
            notification("CONSUMPTION_REQUEST", "com.example.app", "Production"),
            notification("REFUND", "com.example.other", "Production"),
            notification("REFUND", "com.example.app", "Xcode"),
        ] {
            assert!(matches!(
                client().read_notification(body.as_bytes()),
                Some(Notification::Ignored)
            ));
        }
    }

    #[test]
    fn malformed_notifications_are_not_read() {
        assert!(client().read_notification(b"{}").is_none());
        assert!(
            client()
                .read_notification(br#"{"signedPayload":"not-a-jws"}"#)
                .is_none()
        );
    }

    #[test]
    fn only_one_unit_of_the_topup_consumable_of_this_app_is_a_topup() {
        let topup = serde_json::json!({
            "transactionId": "1",
            "bundleId": "com.example.app",
            "productId": PRODUCT_ID,
            "type": CONSUMABLE,
            "quantity": 1,
            "purchaseDate": 0,
            "signedDate": 0,
            "environment": "Production",
        });
        let with = |field: &str, value: serde_json::Value| {
            let mut transaction = topup.clone();
            transaction[field] = value;
            transaction
        };
        for (case, transaction, expected) in [
            ("topup", topup.clone(), true),
            (
                "another app",
                with("bundleId", "com.example.other".into()),
                false,
            ),
            ("another product", with("productId", "other".into()), false),
            (
                "not consumable",
                with("type", "Non-Consumable".into()),
                false,
            ),
            ("two units", with("quantity", 2.into()), false),
            (
                "no quantity",
                with("quantity", serde_json::Value::Null),
                false,
            ),
        ] {
            let transaction: Transaction =
                serde_json::from_value(transaction).expect("transaction should parse");
            assert_eq!(
                is_topup_product(&transaction, "com.example.app"),
                expected,
                "{case}"
            );
        }
    }
}
