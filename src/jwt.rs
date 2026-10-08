//! Apple 向けの JWT の作成と、JWS のペイロードの読み取り (Sign in with Apple・App Store Server API・
//! Google のログインで共用)。

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use p256::ecdsa::SigningKey;
use p256::ecdsa::signature::Signer as _;

/// ヘッダーとクレームを ES256 で署名した JWT (Compact Serialization)。
pub fn sign_es256(
    key: &SigningKey,
    header: &serde_json::Value,
    claims: &serde_json::Value,
) -> String {
    let signing_input = format!(
        "{}.{}",
        URL_SAFE_NO_PAD.encode(header.to_string()),
        URL_SAFE_NO_PAD.encode(claims.to_string())
    );
    let signature: p256::ecdsa::Signature = key.sign(signing_input.as_bytes());
    format!(
        "{signing_input}.{}",
        URL_SAFE_NO_PAD.encode(signature.to_bytes())
    )
}

/// JWS (Compact Serialization、3つに分かれるもの) のペイロードを読む。署名は確かめない
/// (Apple・Google から直接 TLS で受け取ったものか、中身を取り直す前の手がかりにだけ使う)。
pub fn unverified_payload<T: serde::de::DeserializeOwned>(jws: &str) -> Option<T> {
    let mut parts = jws.split('.');
    let (Some(_), Some(payload), Some(_), None) =
        (parts.next(), parts.next(), parts.next(), parts.next())
    else {
        return None;
    };
    serde_json::from_slice(&URL_SAFE_NO_PAD.decode(payload).ok()?).ok()
}
