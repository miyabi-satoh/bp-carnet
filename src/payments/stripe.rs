//! Stripe API の呼び出しと Webhook 署名検証。Stripe 固有の HTTP 形式・型はこのモジュールに閉じ、
//! `payments::ocr_quota` や `api` からは参照しない。

use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

use hmac::{Hmac, KeyInit, Mac};
use serde::Deserialize;
use sha2::Sha256;

use crate::config::{StripeConfig, non_empty_env};
use crate::upstream::{self, http_client};

/// いずれも機微情報のため config.toml には置かず、環境変数から読む (`GoogleLoginClient` と同じ方針)。
pub const SECRET_KEY_ENV: &str = "STRIPE_SECRET_KEY";
pub const WEBHOOK_SECRET_ENV: &str = "STRIPE_WEBHOOK_SECRET";
pub const OCR_TOPUP_PRICE_ID_ENV: &str = "STRIPE_OCR_TOPUP_PRICE_ID";
/// 手動の税率「消費税 10%・税込み」(Tax Rate)。Stripe のアカウントの設定で作る。
pub const TAX_RATE_ID_ENV: &str = "STRIPE_TAX_RATE_ID";

/// インボイスの登録で課税事業者になる日時 (2026-12-01 0時、日本時間)。これより後の Checkout は
/// 項目に税率を付ける (docs/payments.md)。frontend の `$lib/tax.ts` と同じ日時にする。
pub const TAX_INCLUDED_FROM: jiff::Timestamp = jiff::Timestamp::constant(1_796_050_800, 0);

/// 同じ Stripe のアカウントで売るほかの製品の Webhook と見分ける印。
/// Checkout Session の metadata に付ける。
pub const PRODUCT: &str = "bp-carnet";

/// カードの明細で、アカウントの接頭辞 (`AMIIBY.COM` など) の後ろに付ける製品名
/// (docs/payments.md)。
const STATEMENT_DESCRIPTOR_SUFFIX: &str = "BP CARNET";
const STATEMENT_DESCRIPTOR_SUFFIX_KANJI: &str = "BPカルネ";
const STATEMENT_DESCRIPTOR_SUFFIX_KANA: &str = "ビーピーカルネ";

/// Webhook のタイムスタンプ許容誤差 (Stripe 公式ドキュメントの既定値と同じ300秒)。
const WEBHOOK_TOLERANCE_SECS: u64 = 300;

/// Stripe の API エンドポイント。統合テストではモックサーバーに差し替える。
#[derive(Debug, Clone)]
pub struct Endpoints {
    pub api_base: String,
}

impl Default for Endpoints {
    fn default() -> Self {
        Self {
            api_base: "https://api.stripe.com/v1".to_string(),
        }
    }
}

struct Credentials {
    secret_key: String,
    webhook_secret: String,
    price_id: String,
    tax_rate_id: Option<String>,
}

/// Stripe-hosted Checkout の作成と Webhook 検証を行うクライアント。`enabled() == false` の場合、
/// ハンドラ側は買い足し導線・Webhook 受付そのものを拒否する (`OcrService`・`GoogleLoginClient` と同じ設計)。
pub struct StripeClient {
    credentials: Option<Credentials>,
    endpoints: Endpoints,
    success_url: String,
    cancel_url: String,
    /// 税率を付け始める日時。本番は [`TAX_INCLUDED_FROM`]。
    tax_included_from: jiff::Timestamp,
    http: reqwest::Client,
}

// secret_key・webhook_secret をログに出さないよう手書きする (`GoogleLoginClient` と同じ理由)。
impl std::fmt::Debug for StripeClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StripeClient")
            .field("enabled", &self.credentials.is_some())
            .finish()
    }
}

impl StripeClient {
    /// `STRIPE_SECRET_KEY` / `STRIPE_WEBHOOK_SECRET` / `STRIPE_OCR_TOPUP_PRICE_ID`
    /// (空文字列は未設定扱い) と `config.payments.stripe` から構築する。いずれか欠けているか
    /// `enabled = false` なら無効化された状態になる。`STRIPE_TAX_RATE_ID` は有効になる条件ではないが、
    /// [`TAX_INCLUDED_FROM`] からは無いと Checkout を作らない。
    pub fn from_env(config: &StripeConfig) -> Self {
        let secret_key = non_empty_env(SECRET_KEY_ENV);
        let webhook_secret = non_empty_env(WEBHOOK_SECRET_ENV);
        let price_id = non_empty_env(OCR_TOPUP_PRICE_ID_ENV);
        let tax_rate_id = non_empty_env(TAX_RATE_ID_ENV);
        let urls_configured =
            !config.checkout_success_url.is_empty() && !config.checkout_cancel_url.is_empty();

        let credentials = match (secret_key, webhook_secret, price_id) {
            (Some(secret_key), Some(webhook_secret), Some(price_id))
                if config.enabled && urls_configured =>
            {
                Some(Credentials {
                    secret_key,
                    webhook_secret,
                    price_id,
                    tax_rate_id,
                })
            }
            _ => None,
        };

        Self::new(
            credentials,
            Endpoints::default(),
            config.checkout_success_url.clone(),
            config.checkout_cancel_url.clone(),
            TAX_INCLUDED_FROM,
        )
    }

    /// 常に無効化されたクライアント。統合テストのように、実行環境の環境変数に依存せず
    /// 「Stripe 決済無効時」の挙動を明示的に検証したい場合に使う。
    pub fn disabled() -> Self {
        Self::new(
            None,
            Endpoints::default(),
            String::new(),
            String::new(),
            TAX_INCLUDED_FROM,
        )
    }

    /// 統合テストから、モックサーバーの URL で有効化されたクライアントを作る。税率は付けない
    /// (テストを走らせる日で結果が変わらないように。付けるときは [`Self::with_tax_rate`])。
    /// `#[cfg(test)]` にしないのは `GoogleLoginClient::for_test` と同じ理由
    /// (`tests/api.rs` は別クレートとしてコンパイルされるため)。
    pub fn for_test(
        secret_key: impl Into<String>,
        webhook_secret: impl Into<String>,
        price_id: impl Into<String>,
        success_url: impl Into<String>,
        cancel_url: impl Into<String>,
        endpoints: Endpoints,
    ) -> Self {
        Self::new(
            Some(Credentials {
                secret_key: secret_key.into(),
                webhook_secret: webhook_secret.into(),
                price_id: price_id.into(),
                tax_rate_id: None,
            }),
            endpoints,
            success_url.into(),
            cancel_url.into(),
            jiff::Timestamp::MAX,
        )
    }

    /// 統合テストから、税率と付け始める日時を差し替える。
    pub fn with_tax_rate(
        mut self,
        tax_rate_id: Option<&str>,
        tax_included_from: jiff::Timestamp,
    ) -> Self {
        if let Some(creds) = self.credentials.as_mut() {
            creds.tax_rate_id = tax_rate_id.map(str::to_string);
        }
        self.tax_included_from = tax_included_from;
        self
    }

    fn new(
        credentials: Option<Credentials>,
        endpoints: Endpoints,
        success_url: String,
        cancel_url: String,
        tax_included_from: jiff::Timestamp,
    ) -> Self {
        Self {
            credentials,
            endpoints,
            success_url,
            cancel_url,
            tax_included_from,
            http: http_client(),
        }
    }

    pub fn enabled(&self) -> bool {
        self.credentials.is_some()
    }

    /// 有効なのに税率が入っていない。起動時に知らせるために使う。
    pub fn tax_rate_missing(&self) -> bool {
        self.credentials
            .as_ref()
            .is_some_and(|c| c.tax_rate_id.is_none())
    }

    /// サーバーに設定されたOCR枠買い足しのPrice ID。Webhook処理時に、実際に決済されたPriceが
    /// これと一致するかを再検証するために使う。
    pub fn price_id(&self) -> Option<&str> {
        self.credentials.as_ref().map(|c| c.price_id.as_str())
    }

    /// Checkout Session を作成する。クライアントからは金額・Price ID・対象ユーザーを受け取らず、
    /// サーバー設定の Price ID とログイン中のユーザーだけで組み立てる。
    /// 課税事業者になった後に税率が入っていなければ、適格簡易請求書にならない領収書を出さないよう作らない。
    pub async fn create_checkout_session(
        &self,
        user_id: i64,
        customer_email: Option<&str>,
    ) -> Result<CheckoutSession, StripeError> {
        let creds = self.credentials.as_ref().ok_or(StripeError::Disabled)?;
        let tax_rate_id = if jiff::Timestamp::now() >= self.tax_included_from {
            Some(
                creds
                    .tax_rate_id
                    .as_deref()
                    .ok_or(StripeError::TaxRateMissing)?,
            )
        } else {
            None
        };
        let user_id_text = user_id.to_string();
        let mut form: Vec<(&str, &str)> = vec![
            ("mode", "payment"),
            ("line_items[0][price]", creds.price_id.as_str()),
            ("line_items[0][quantity]", "1"),
            ("success_url", self.success_url.as_str()),
            ("cancel_url", self.cancel_url.as_str()),
            ("client_reference_id", user_id_text.as_str()),
            ("metadata[user_id]", user_id_text.as_str()),
            ("metadata[product]", PRODUCT),
            // Managed Payments (Link の代理販売) を外す。税の代行は海外の売上にしか効かず、
            // 日本だけで売るこのアプリでは手数料の上乗せと Link の返金ポリシーの優先だけが残るため。
            // 指定しないとアカウント側の既定で有効になる (2026-09-25 にサンドボックスで確認)。
            ("managed_payments[enabled]", "false"),
            (
                "payment_intent_data[statement_descriptor_suffix]",
                STATEMENT_DESCRIPTOR_SUFFIX,
            ),
            (
                "payment_method_options[card][statement_descriptor_suffix_kanji]",
                STATEMENT_DESCRIPTOR_SUFFIX_KANJI,
            ),
            (
                "payment_method_options[card][statement_descriptor_suffix_kana]",
                STATEMENT_DESCRIPTOR_SUFFIX_KANA,
            ),
        ];
        if let Some(tax_rate_id) = tax_rate_id {
            form.push(("line_items[0][tax_rates][0]", tax_rate_id));
        }
        if let Some(email) = customer_email {
            form.push(("customer_email", email));
        }

        let url = format!("{}/checkout/sessions", self.endpoints.api_base);
        let response = self
            .http
            .post(&url)
            .bearer_auth(&creds.secret_key)
            .form(&form)
            .send()
            .await?;
        upstream::parse_success(response, "stripe", "api")
            .await
            .map_err(StripeError::from)
    }

    /// Webhook 処理時に、metadata を受け取るだけでなく Session を Stripe API から取得して
    /// 支払状態・Price・ユーザーIDを再検証するために使う (同上ドキュメント)。
    pub async fn retrieve_checkout_session(
        &self,
        session_id: &str,
    ) -> Result<CheckoutSession, StripeError> {
        let creds = self.credentials.as_ref().ok_or(StripeError::Disabled)?;
        let url = format!("{}/checkout/sessions/{session_id}", self.endpoints.api_base);
        let response = self
            .http
            .get(&url)
            .bearer_auth(&creds.secret_key)
            // Price ID・数量まで再検証するため (`grant_ocr_topup`)。
            .query(&[("expand[]", "line_items")])
            .send()
            .await?;
        upstream::parse_success(response, "stripe", "api")
            .await
            .map_err(StripeError::from)
    }

    /// `charge.refunded`・`charge.dispute.created` は Charge 単位で届き、対応する
    /// Checkout Session ID を持たないため、支払いの PaymentIntent ID から Session を探す。
    pub async fn find_checkout_session_by_payment_intent(
        &self,
        payment_intent_id: &str,
    ) -> Result<Option<CheckoutSession>, StripeError> {
        let creds = self.credentials.as_ref().ok_or(StripeError::Disabled)?;
        let url = format!("{}/checkout/sessions", self.endpoints.api_base);
        let response = self
            .http
            .get(&url)
            .bearer_auth(&creds.secret_key)
            .query(&[("payment_intent", payment_intent_id)])
            .send()
            .await?;
        let list: CheckoutSessionList = upstream::parse_success(response, "stripe", "api").await?;
        Ok(list.data.into_iter().next())
    }

    /// Webhook の `Stripe-Signature` ヘッダーを検証する。生のリクエストボディ (JSON化・
    /// 再シリアライズ前) で検証すること。タイムスタンプが許容誤差を超えている場合も拒否する
    /// (リプレイ攻撃対策、Stripe公式ドキュメント推奨値の300秒)。
    pub fn verify_webhook_signature(
        &self,
        payload: &[u8],
        signature_header: &str,
    ) -> Result<(), StripeError> {
        let creds = self.credentials.as_ref().ok_or(StripeError::Disabled)?;
        let (timestamp, signatures) =
            parse_signature_header(signature_header).ok_or(StripeError::InvalidSignature)?;

        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| StripeError::InvalidSignature)?
            .as_secs();
        let timestamp_value: u64 = timestamp
            .parse()
            .map_err(|_| StripeError::InvalidSignature)?;
        if now.abs_diff(timestamp_value) > WEBHOOK_TOLERANCE_SECS {
            return Err(StripeError::SignatureTimestampOutOfRange);
        }

        let mut mac = Hmac::<Sha256>::new_from_slice(creds.webhook_secret.as_bytes())
            .map_err(|_| StripeError::InvalidSignature)?;
        mac.update(timestamp.as_bytes());
        mac.update(b".");
        mac.update(payload);

        let matches = signatures.iter().any(|signature| {
            hex::decode(signature)
                .map(|expected| mac.clone().verify_slice(&expected).is_ok())
                .unwrap_or(false)
        });
        if matches {
            Ok(())
        } else {
            Err(StripeError::InvalidSignature)
        }
    }
}

/// `Stripe-Signature: t=<timestamp>,v1=<signature>[,v1=<signature>...]` を分解する。
/// ローテーション中は複数の `v1` が並びうるため、いずれか1つが一致すれば検証成功とする
/// (Stripe公式ドキュメント「署名の検証」)。
fn parse_signature_header(header: &str) -> Option<(&str, Vec<&str>)> {
    let mut timestamp = None;
    let mut signatures = Vec::new();
    for item in header.split(',') {
        let (key, value) = item.split_once('=')?;
        match key.trim() {
            "t" => timestamp = Some(value.trim()),
            "v1" => signatures.push(value.trim()),
            _ => {}
        }
    }
    let timestamp = timestamp?;
    if signatures.is_empty() {
        return None;
    }
    Some((timestamp, signatures))
}

#[derive(Debug, thiserror::Error)]
pub enum StripeError {
    #[error("stripe integration is not configured")]
    Disabled,
    #[error("stripe tax rate is not configured")]
    TaxRateMissing,
    #[error("failed to call the stripe api")]
    Request(#[source] reqwest::Error),
    #[error("stripe returned status {0}")]
    UpstreamStatus(u16),
    #[error("failed to parse the stripe api response")]
    ParseResponse,
    #[error("stripe webhook signature is invalid")]
    InvalidSignature,
    #[error("stripe webhook timestamp is outside the allowed tolerance")]
    SignatureTimestampOutOfRange,
}

/// `without_url()`: エラーメッセージに URL を含めない (ログに出るため、`ProviderError`・`OcrError` と同じ方針)。
impl From<reqwest::Error> for StripeError {
    fn from(err: reqwest::Error) -> Self {
        Self::Request(err.without_url())
    }
}

impl From<upstream::Error> for StripeError {
    fn from(err: upstream::Error) -> Self {
        match err {
            upstream::Error::Request(err) => Self::from(err),
            upstream::Error::UpstreamStatus(status) => Self::UpstreamStatus(status),
            upstream::Error::ParseResponse => Self::ParseResponse,
        }
    }
}

/// Checkout Session。使わないフィールドは受け取らない。
#[derive(Debug, Clone, Deserialize)]
pub struct CheckoutSession {
    pub id: String,
    /// Session 作成直後の redirect 先。取得系 API 呼び出しでは返らないことがある。
    pub url: Option<String>,
    /// `"open"` | `"complete"` | `"expired"`。
    pub status: String,
    /// `"paid"` | `"unpaid"` | `"no_payment_required"`。
    pub payment_status: String,
    pub mode: String,
    pub client_reference_id: Option<String>,
    pub payment_intent: Option<String>,
    /// 最小通貨単位 (JPYはゼロ小数通貨なので円そのもの)。Webhook処理時に、実際に決済された額が
    /// サーバーが設定したPriceどおりかを再検証するために使う。
    pub amount_total: Option<i64>,
    pub currency: Option<String>,
    /// `retrieve_checkout_session` が `expand[]=line_items` を付けて取得したときだけ入る。
    /// Price ID・数量を再検証するために使う (`grant_ocr_topup`)。
    pub line_items: Option<LineItemList>,
    #[serde(default)]
    pub metadata: HashMap<String, String>,
    pub customer_details: Option<CustomerDetails>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CustomerDetails {
    pub address: Option<Address>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Address {
    /// ISO 3166-1 alpha-2。カードの支払いでは、Checkout が住所を集めない設定でも入る。
    pub country: Option<String>,
}

impl CheckoutSession {
    /// 買い手が国内か。Checkout の住所の国で決める。無ければ `None`。
    pub fn buyer_is_domestic(&self) -> Option<bool> {
        let country = self
            .customer_details
            .as_ref()?
            .address
            .as_ref()?
            .country
            .as_deref()?;
        Some(country == "JP")
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct LineItemList {
    pub data: Vec<LineItem>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct LineItem {
    pub price: Option<LineItemPrice>,
    pub quantity: Option<i64>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct LineItemPrice {
    pub id: String,
}

#[derive(Debug, Deserialize)]
struct CheckoutSessionList {
    data: Vec<CheckoutSession>,
}

/// Webhook が届ける Event。`data.object` の形はイベント種別ごとに違うため、
/// 呼び出し側 (`payments::dispatch_stripe_event`) が種別を見てから解釈する。
#[derive(Debug, Deserialize)]
pub struct WebhookEvent {
    pub id: String,
    #[serde(rename = "type")]
    pub event_type: String,
    /// Stripeがこのイベントを生成したUnix時刻 (秒)。購入日時 (`purchased_at`) は、Webhook配送・
    /// 処理の遅延に左右されないよう、これを使う (処理時刻の `now()` は使わない)。
    pub created: i64,
    pub data: WebhookEventData,
}

#[derive(Debug, Deserialize)]
pub struct WebhookEventData {
    pub object: serde_json::Value,
}

/// `charge.refunded`・`charge.dispute.created` の `data.object`。使わないフィールドは受け取らない。
#[derive(Debug, Deserialize)]
pub struct Charge {
    pub payment_intent: Option<String>,
}

/// テスト (`src`内・`tests/api.rs`) から、有効な `Stripe-Signature` ヘッダーを作るために使う。
/// `#[cfg(test)]` にしない理由は `StripeClient::for_test` と同じ (`tests/api.rs` は別クレート)。
pub fn sign_webhook_for_test(webhook_secret: &str, payload: &[u8]) -> String {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock should be after epoch")
        .as_secs()
        .to_string();
    let mut mac = Hmac::<Sha256>::new_from_slice(webhook_secret.as_bytes())
        .expect("secret should be a valid hmac key");
    mac.update(timestamp.as_bytes());
    mac.update(b".");
    mac.update(payload);
    let signature = hex::encode(mac.finalize().into_bytes());
    format!("t={timestamp},v1={signature}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tax_is_included_from_the_invoice_registration_date_in_japan() {
        let expected: jiff::Timestamp = "2026-12-01T00:00:00+09:00"
            .parse()
            .expect("timestamp should parse");
        assert_eq!(TAX_INCLUDED_FROM, expected);
    }

    fn sign(secret: &str, timestamp: &str, payload: &[u8]) -> String {
        let mut mac = Hmac::<Sha256>::new_from_slice(secret.as_bytes())
            .expect("secret should be a valid hmac key");
        mac.update(timestamp.as_bytes());
        mac.update(b".");
        mac.update(payload);
        hex::encode(mac.finalize().into_bytes())
    }

    fn client_for_signature_tests() -> StripeClient {
        StripeClient::for_test(
            "sk_test_x",
            "whsec_test",
            "price_test",
            "https://example.test/success",
            "https://example.test/cancel",
            Endpoints::default(),
        )
    }

    #[test]
    fn disabled_client_rejects_signature_verification() {
        let client = StripeClient::disabled();
        assert!(!client.enabled());
        assert!(matches!(
            client.verify_webhook_signature(b"{}", "t=1,v1=abc"),
            Err(StripeError::Disabled)
        ));
    }

    #[test]
    fn valid_signature_with_current_timestamp_is_accepted() {
        let client = client_for_signature_tests();
        let payload = br#"{"id":"evt_1"}"#;
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock should be after epoch")
            .as_secs()
            .to_string();
        let signature = sign("whsec_test", &timestamp, payload);
        let header = format!("t={timestamp},v1={signature}");
        assert!(client.verify_webhook_signature(payload, &header).is_ok());
    }

    #[test]
    fn tampered_payload_is_rejected() {
        let client = client_for_signature_tests();
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock should be after epoch")
            .as_secs()
            .to_string();
        let signature = sign("whsec_test", &timestamp, br#"{"id":"evt_1"}"#);
        let header = format!("t={timestamp},v1={signature}");
        assert!(matches!(
            client.verify_webhook_signature(br#"{"id":"evt_2"}"#, &header),
            Err(StripeError::InvalidSignature)
        ));
    }

    #[test]
    fn old_timestamp_is_rejected_even_with_a_valid_signature() {
        let client = client_for_signature_tests();
        let payload = br#"{"id":"evt_1"}"#;
        let old_timestamp = (SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock should be after epoch")
            .as_secs())
            - WEBHOOK_TOLERANCE_SECS
            - 1;
        let timestamp = old_timestamp.to_string();
        let signature = sign("whsec_test", &timestamp, payload);
        let header = format!("t={timestamp},v1={signature}");
        assert!(matches!(
            client.verify_webhook_signature(payload, &header),
            Err(StripeError::SignatureTimestampOutOfRange)
        ));
    }

    #[test]
    fn missing_signature_header_parts_are_rejected() {
        let client = client_for_signature_tests();
        assert!(matches!(
            client.verify_webhook_signature(b"{}", "t=123"),
            Err(StripeError::InvalidSignature)
        ));
        assert!(matches!(
            client.verify_webhook_signature(b"{}", "v1=abc"),
            Err(StripeError::InvalidSignature)
        ));
    }
}
