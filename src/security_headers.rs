//! すべての応答に付けるセキュリティヘッダー (docs/architecture.md「セキュリティヘッダー」)。
//!
//! 画面 (静的ファイル) の Content-Security-Policy は、index.html のインラインスクリプトのハッシュが要るので
//! `crate::static_files` が [`page_csp`] で付ける。ここでは、CSP の無い応答 (API など) に API 用の CSP を付ける。

use axum::extract::{Request, State};
use axum::http::{HeaderValue, header};
use axum::middleware::Next;
use axum::response::Response;
use sha2::{Digest, Sha256};

/// HSTS の期間 (2年)。Mozilla の推奨。
const HSTS: HeaderValue = HeaderValue::from_static("max-age=63072000");

/// API の応答は画面として描かれないので、何も読ませず、枠にも入れさせない。
const API_CSP: HeaderValue = HeaderValue::from_static("default-src 'none'; frame-ancestors 'none'");

/// Cloudflare Web Analytics のスクリプトの出どころ (`frontend/src/lib/web-analytics.ts` の `BEACON_SRC`)。
const ANALYTICS_SCRIPT_SRC: &str = "https://static.cloudflareinsights.com";
/// Cloudflare Web Analytics のスクリプトが計測を送る先。
const ANALYTICS_CONNECT_SRC: &str = "https://cloudflareinsights.com";

#[derive(Debug, Clone, Copy)]
pub struct Settings {
    /// https で公開している (`[session] secure_cookie`)。HSTS を付けるかに使う。
    pub secure: bool,
}

pub async fn add(State(settings): State<Settings>, request: Request, next: Next) -> Response {
    let mut response = next.run(request).await;
    let headers = response.headers_mut();
    headers.insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
    headers.insert(
        header::REFERRER_POLICY,
        HeaderValue::from_static("strict-origin-when-cross-origin"),
    );
    if settings.secure {
        headers.insert(header::STRICT_TRANSPORT_SECURITY, HSTS);
    }
    if !headers.contains_key(header::CONTENT_SECURITY_POLICY) {
        headers.insert(header::CONTENT_SECURITY_POLICY, API_CSP);
    }
    response
}

/// 画面 (静的ファイル・Service Worker のスクリプト) の CSP。`html` は index.html の中身で、インラインスクリプトの
/// ハッシュを `script-src` に入れる (起動のスクリプトはビルドのたびに変わるため、返すたびに求める)。
/// `analytics` は、アクセス解析を有効にしているか (`[server] cloudflare_analytics_token`)。
pub fn page_csp(html: Option<&str>, analytics: bool) -> HeaderValue {
    let mut script_src = vec!["'self'".to_string()];
    script_src.extend(
        html.into_iter()
            .flat_map(inline_scripts)
            .map(|script| format!("'sha256-{}'", sha256_base64(script))),
    );
    let mut connect_src = vec!["'self'"];
    if analytics {
        script_src.push(ANALYTICS_SCRIPT_SRC.to_string());
        connect_src.push(ANALYTICS_CONNECT_SRC);
    }
    let policy = [
        "default-src 'self'".to_string(),
        format!("script-src {}", script_src.join(" ")),
        // Svelte が要素に直接スタイルを書くため。
        "style-src 'self' 'unsafe-inline'".to_string(),
        // プロフィール画像 (Google・LINE の https の URL) と、写真の画面の blob: の画像。
        "img-src 'self' data: blob: https:".to_string(),
        // フォントの CSS が小さなフォントを data: で埋め込んでいるため。
        "font-src 'self' data:".to_string(),
        format!("connect-src {}", connect_src.join(" ")),
        "frame-ancestors 'none'".to_string(),
        "object-src 'none'".to_string(),
        "base-uri 'self'".to_string(),
        "form-action 'self'".to_string(),
    ]
    .join("; ");
    HeaderValue::from_str(&policy).expect("CSP は ASCII のはず")
}

/// `src` を持たない `<script>` の中身。index.html は自分たちのビルドの出力なので、属性に `>` を含まない前提で読む。
fn inline_scripts(html: &str) -> Vec<&str> {
    let mut scripts = Vec::new();
    let mut rest = html;
    while let Some(start) = rest.find("<script") {
        let after_tag = &rest[start..];
        let Some(open_end) = after_tag.find('>') else {
            break;
        };
        let attributes = &after_tag[..open_end];
        let body = &after_tag[open_end + 1..];
        let Some(close) = body.find("</script>") else {
            break;
        };
        if !attributes
            .split_ascii_whitespace()
            .any(|attribute| attribute.starts_with("src="))
        {
            scripts.push(&body[..close]);
        }
        rest = &body[close..];
    }
    scripts
}

fn sha256_base64(value: &str) -> String {
    base64::Engine::encode(
        &base64::engine::general_purpose::STANDARD,
        Sha256::digest(value.as_bytes()),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn csp(html: Option<&str>, analytics: bool) -> String {
        page_csp(html, analytics)
            .to_str()
            .expect("ASCII のはず")
            .to_string()
    }

    /// ハッシュはブラウザと同じく、タグの間の文字列そのもの (空白を含む) の SHA-256 を base64 にしたもの。
    #[test]
    fn page_csp_allows_only_the_inline_scripts_without_src() {
        let html = r#"<head><script>alert(1)</script><script type="module" src="/a.js"></script>
<script data-src="x">lazy()</script></head>
<body><script>
  start();
</script></body>"#;
        let policy = csp(Some(html), false);
        assert!(
            policy.contains(&format!("'sha256-{}'", sha256_base64("alert(1)"))),
            "{policy}"
        );
        assert!(
            policy.contains(&format!("'sha256-{}'", sha256_base64("\n  start();\n"))),
            "{policy}"
        );
        // `data-src` は外のスクリプトの指定ではないので、中身を許す。
        assert!(
            policy.contains(&format!("'sha256-{}'", sha256_base64("lazy()"))),
            "{policy}"
        );
        assert_eq!(policy.matches("'sha256-").count(), 3, "{policy}");
    }

    /// 既知の値: `alert(1)` の SHA-256 の base64 (`echo -n 'alert(1)' | openssl dgst -sha256 -binary | base64`)。
    #[test]
    fn sha256_base64_uses_the_standard_alphabet() {
        assert_eq!(
            sha256_base64("alert(1)"),
            "bhHHL3z2vDgxUt0W3dWQOrprscmda2Y5pLsLg4GF+pI="
        );
    }

    #[test]
    fn page_csp_allows_cloudflare_only_when_analytics_is_enabled() {
        let without = csp(None, false);
        assert!(!without.contains("cloudflareinsights"), "{without}");
        let with = csp(None, true);
        assert!(
            with.contains("script-src 'self' https://static.cloudflareinsights.com"),
            "{with}"
        );
        assert!(
            with.contains("connect-src 'self' https://cloudflareinsights.com"),
            "{with}"
        );
    }

    #[test]
    fn page_csp_forbids_framing_and_plugins() {
        let policy = csp(None, false);
        for directive in [
            "default-src 'self'",
            "frame-ancestors 'none'",
            "object-src 'none'",
            "base-uri 'self'",
            "form-action 'self'",
        ] {
            assert!(policy.contains(directive), "{directive}: {policy}");
        }
    }
}
