//! 外部サービスのトークンを DB に置くための暗号化 (XChaCha20-Poly1305)。
//!
//! 鍵は環境変数で渡す秘密 (LINE のチャネルシークレット等) から導く。DB とそのバックアップが
//! 漏れても、鍵は一緒に漏れないようにするため (`data/` に置くセッション鍵は使わない)。
//! 秘密が変わると以前のトークンは復号できなくなるが、呼び出し側はそれを「使えない」として扱う。

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use chacha20poly1305::aead::{Aead, Generate};
use chacha20poly1305::{Key, KeyInit, XChaCha20Poly1305, XNonce};
use sha2::{Digest, Sha256};

const NONCE_LEN: usize = 24;

pub struct TokenCipher {
    cipher: XChaCha20Poly1305,
}

// 鍵をログに出さない。
impl std::fmt::Debug for TokenCipher {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TokenCipher").finish_non_exhaustive()
    }
}

impl TokenCipher {
    /// `secret` から鍵を導く。`purpose` は同じ秘密を別の用途に使ったときに鍵を分けるためのもの。
    pub fn derive(purpose: &str, secret: &str) -> Self {
        let digest = Sha256::new()
            .chain_update(purpose.as_bytes())
            .chain_update([0u8])
            .chain_update(secret.as_bytes())
            .finalize();
        let key = Key::try_from(digest.as_slice()).expect("SHA-256 の出力は32バイト");
        Self {
            cipher: XChaCha20Poly1305::new(&key),
        }
    }

    /// 暗号化して、nonce と暗号文を続けた URL-safe base64 にする。
    pub fn seal(&self, plaintext: &str) -> String {
        let nonce = XNonce::generate();
        let ciphertext = self
            .cipher
            .encrypt(&nonce, plaintext.as_bytes())
            .expect("メモリ上の暗号化は失敗しない");
        let mut packed = nonce.to_vec();
        packed.extend_from_slice(&ciphertext);
        URL_SAFE_NO_PAD.encode(packed)
    }

    /// [`Self::seal`] の値を戻す。形が壊れている・鍵が違う・改ざんされている場合は `None`。
    pub fn open(&self, sealed: &str) -> Option<String> {
        let packed = URL_SAFE_NO_PAD.decode(sealed).ok()?;
        if packed.len() < NONCE_LEN {
            return None;
        }
        let (nonce, ciphertext) = packed.split_at(NONCE_LEN);
        let nonce = XNonce::try_from(nonce).ok()?;
        let plaintext = self.cipher.decrypt(&nonce, ciphertext).ok()?;
        String::from_utf8(plaintext).ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn open_returns_what_was_sealed() {
        let cipher = TokenCipher::derive("test", "secret");
        let sealed = cipher.seal("refresh-token");
        assert_ne!(sealed, "refresh-token");
        assert_eq!(cipher.open(&sealed).as_deref(), Some("refresh-token"));
    }

    #[test]
    fn sealing_twice_gives_different_values() {
        let cipher = TokenCipher::derive("test", "secret");
        assert_ne!(cipher.seal("token"), cipher.seal("token"));
    }

    #[test]
    fn open_rejects_a_different_key_or_purpose() {
        let sealed = TokenCipher::derive("test", "secret").seal("token");
        assert_eq!(TokenCipher::derive("test", "other").open(&sealed), None);
        assert_eq!(TokenCipher::derive("other", "secret").open(&sealed), None);
    }

    #[test]
    fn open_rejects_broken_values() {
        let cipher = TokenCipher::derive("test", "secret");
        let mut sealed = cipher.seal("token");
        sealed.pop();
        assert_eq!(cipher.open(&sealed), None);
        assert_eq!(cipher.open("short"), None);
        assert_eq!(cipher.open("not base64!"), None);
    }
}
