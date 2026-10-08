//! 推測されてはならない値 (セッション ID・OAuth の state・メール内リンクのトークン等) の生成と、
//! その照合に使うハッシュ。

use rand::{Rng, RngExt};
use sha2::{Digest, Sha256};

/// `n_bytes` バイトの乱数を URL-safe base64 (パディング無し) にする。
pub fn random_url_safe(n_bytes: usize) -> String {
    let mut bytes = vec![0u8; n_bytes];
    rand::rng().fill_bytes(&mut bytes);
    base64::Engine::encode(&base64::engine::general_purpose::URL_SAFE_NO_PAD, bytes)
}

/// 英数字だけの乱数の文字列 (`len` 文字)。英数字しか受け付けない相手 (LINE の `state` 等) に使う。
pub fn random_alphanumeric(len: usize) -> String {
    rand::rng()
        .sample_iter(rand::distr::Alphanumeric)
        .take(len)
        .map(char::from)
        .collect()
}

/// 乱数による UUID (version 4) を小文字で書く。StoreKit の `appAccountToken` のように UUID を求める相手に使う。
pub fn random_uuid_v4() -> String {
    let mut bytes = [0u8; 16];
    rand::rng().fill_bytes(&mut bytes);
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    let hex = hex::encode(bytes);
    format!(
        "{}-{}-{}-{}-{}",
        &hex[0..8],
        &hex[8..12],
        &hex[12..16],
        &hex[16..20],
        &hex[20..32]
    )
}

/// SHA-256 を URL-safe base64 (パディング無し) にする。
pub fn sha256_url_safe(value: &str) -> String {
    base64::Engine::encode(
        &base64::engine::general_purpose::URL_SAFE_NO_PAD,
        Sha256::digest(value.as_bytes()),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 32 バイトは base64url (パディング無し) で 43 文字になる。PKCE の `code_verifier` が
    /// RFC 7636 の 43-128 文字に収まる根拠。
    #[test]
    fn random_url_safe_encodes_without_padding() {
        let value = random_url_safe(32);
        assert_eq!(value.len(), 43);
        assert!(
            value
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
        );
    }

    #[test]
    fn random_alphanumeric_has_the_length_and_only_alphanumerics() {
        let value = random_alphanumeric(43);
        assert_eq!(value.len(), 43);
        assert!(value.chars().all(|c| c.is_ascii_alphanumeric()));
    }

    /// RFC 7636 Appendix B のテストベクタ。
    #[test]
    fn sha256_url_safe_matches_rfc7636_example() {
        assert_eq!(
            sha256_url_safe("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk"),
            "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
        );
    }
}
