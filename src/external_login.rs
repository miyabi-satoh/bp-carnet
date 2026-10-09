//! 外部のアカウントでのログイン (Google: `crate::google_login`、LINE: `crate::line_login`、
//! Apple: `crate::apple_login`) のクライアントで共通の部品。
//!
//! どれも Authorization Code で、プロバイダーのエンドポイントと reqwest で直接やりとりする。

use crate::apple_login::AppleLoginClient;
use crate::line_login::LineLoginClient;
use crate::oauth_identity::{Provider, RevocationTokens};
use crate::token::{random_alphanumeric, random_url_safe, sha256_url_safe};
use crate::upstream;

#[derive(Debug, thiserror::Error)]
pub enum ProviderError {
    #[error("the login provider is not configured")]
    Disabled,
    #[error("failed to call the login provider")]
    Request(#[source] reqwest::Error),
    #[error("the login provider returned status {0}")]
    UpstreamStatus(u16),
    #[error("failed to parse the login provider's response")]
    ParseResponse,
    /// ID トークンの発行元・宛先・期限・`nonce` のどれかが、このログインのものと合わない。
    #[error("the ID token does not match this login")]
    IdTokenMismatch,
}

impl From<reqwest::Error> for ProviderError {
    /// `without_url()`: エラーメッセージに URL を含めない (ログに出るため)。
    fn from(err: reqwest::Error) -> Self {
        Self::Request(err.without_url())
    }
}

impl From<upstream::Error> for ProviderError {
    fn from(err: upstream::Error) -> Self {
        match err {
            upstream::Error::Request(err) => Self::from(err),
            upstream::Error::UpstreamStatus(status) => Self::UpstreamStatus(status),
            upstream::Error::ParseResponse => Self::ParseResponse,
        }
    }
}

/// 退会・連携の解除で、提供元との連携を取り消せなかった理由。
#[derive(Debug, thiserror::Error)]
pub enum RevokeError {
    /// 取り消しに使えるトークンを持っていない。
    #[error("no token is stored")]
    NoToken,
    /// 提供元の設定が無い (ログインをやめた構成に、以前のトークンが残っている)。
    #[error("the login provider is not configured")]
    Disabled,
    /// 保存したトークンを復号できない (鍵を導く秘密を発行し直した等)。
    #[error("the stored token cannot be decrypted")]
    Undecryptable,
    #[error(transparent)]
    Upstream(#[from] ProviderError),
}

impl From<reqwest::Error> for RevokeError {
    fn from(err: reqwest::Error) -> Self {
        Self::Upstream(err.into())
    }
}

impl From<upstream::Error> for RevokeError {
    fn from(err: upstream::Error) -> Self {
        Self::Upstream(err.into())
    }
}

/// 退会・連携の解除で、提供元との連携を取り消す (docs/authentication.md)。
/// Google は取り消さない (トークンを持たない)。
pub async fn revoke_link(
    line: &LineLoginClient,
    apple: &AppleLoginClient,
    provider: Provider,
    tokens: &RevocationTokens,
) -> Result<(), RevokeError> {
    match provider {
        Provider::Google => Ok(()),
        Provider::Line => line.revoke(tokens).await,
        Provider::Apple => {
            let sealed = tokens
                .refresh_token
                .as_deref()
                .ok_or(RevokeError::NoToken)?;
            apple.revoke(sealed).await
        }
    }
}

/// [`revoke_link`] を呼び、結果をログに残す。取り消せなくても呼び出し側の処理は止めない
/// (`failure_message` は、取り消せなかったときに続ける処理を添えたログの文言)。
pub async fn revoke_link_logged(
    line: &LineLoginClient,
    apple: &AppleLoginClient,
    provider: Provider,
    tokens: &RevocationTokens,
    user_id: i64,
    failure_message: &str,
) {
    match revoke_link(line, apple, provider, tokens).await {
        Ok(()) => tracing::info!(
            user_id,
            provider = provider.as_str(),
            "提供元との連携を取り消しました"
        ),
        Err(err) => tracing::warn!(
            user_id,
            provider = provider.as_str(),
            error = %crate::error_chain_line(&err),
            "{failure_message}"
        ),
    }
}

/// ADR: `state`・`nonce` の長さ。英数字 (62 種) の 43 文字で約 256bit になり、PKCE の
/// `code_verifier` (32 バイト) と同じ強さにそろう。LINE は英数字以外を受け付けないことがあるため、
/// 英数字だけで作る。
const STATE_NONCE_LEN: usize = 43;

/// `state` を作る (Google・LINE・Apple)。
pub fn generate_state() -> String {
    random_alphanumeric(STATE_NONCE_LEN)
}

/// ID トークンを、この認可リクエストのものと結び付ける `nonce` を作る (LINE・Apple、アプリの
/// ログインの `nonce` も含む)。
pub fn generate_nonce() -> String {
    random_alphanumeric(STATE_NONCE_LEN)
}

/// PKCE の `code_verifier` を生成する。RFC 7636 は 43-128 文字を要求するが、32 バイト
/// (256bit) を base64url エンコードすると 43 文字になり範囲内に収まる。
pub fn generate_code_verifier() -> String {
    random_url_safe(32)
}

/// PKCE `code_challenge` (S256 方式) を計算する。
pub fn code_challenge_s256(verifier: &str) -> String {
    sha256_url_safe(verifier)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generated_verifier_is_url_safe_long_enough_and_not_repeated() {
        let verifier = generate_code_verifier();
        // RFC 7636: code_verifier は 43-128 文字。
        assert!(verifier.len() >= 43);
        assert!(
            verifier
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_'),
            "{verifier}"
        );
        assert_ne!(verifier, generate_code_verifier());
    }

    #[test]
    fn state_and_nonce_are_alphanumeric() {
        for value in [generate_state(), generate_nonce()] {
            assert!(value.chars().all(|c| c.is_ascii_alphanumeric()), "{value}");
        }
    }

    #[test]
    fn code_challenge_matches_rfc7636_appendix_b_vector() {
        // RFC 7636 Appendix B の既知ベクトル。
        let verifier = "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk";
        let challenge = code_challenge_s256(verifier);
        assert_eq!(challenge, "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM");
    }
}
