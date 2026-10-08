//! Google・LINE のログイン開始からコールバックまでの間、CSRF (state) 検証用の `state` と
//! PKCE の `code_verifier` (LINE は ID トークンの `nonce` も) を保持する専用 Cookie。
//! プロバイダーごとに名前を分け、両方を並行して始めても互いを上書きしないようにする。
//!
//! 既存のセッション Cookie (`src/session.rs`) とは別に独立して発行・破棄する。理由:
//! Google・LINE → 自ホストへのコールバックはクロスサイトのトップレベル GET ナビゲーションであり、
//! tower-sessions のデフォルト `SameSite=Strict` (`src/csrf.rs` のモジュールコメント参照) の
//! セッション Cookie はこの時点では送信されない。このCookieは state 検証専用でセッション
//! 確立には一切関与しないため、`SameSite=Lax` にしても既存の CSRF 防御モデル
//! (`crate::csrf::same_origin_check` + セッション Cookie の `SameSite=Strict`) への影響はない。
//!
//! 新規 crate は追加していない: `tower_sessions::cookie` が `cookie` crate 0.18.2 を
//! そのまま再エクスポートしており (`tower-sessions-0.14.0/src/lib.rs`)、`session.rs` が
//! `Key` として使っている実体そのものである。

use axum::http::{HeaderMap, HeaderValue, header};
use time::Duration;
use tower_sessions::cookie::{Cookie, SameSite};

use crate::oauth_identity::Provider;

/// Cookie の名前とパス。
///
/// Secure を付けるときは、セッション Cookie (`crate::session::cookie_name`) と同じ理由で `__Host-` を付ける。
/// 別のサブドメインから `state` を送り込まれると、攻撃者のアカウントでログインさせる (login CSRF) 余地が出るため。
/// `__Host-` は `Path=/` が必須なので、そのときだけパスを広げる (10分で失効し、コールバックで消す)。
/// Secure を付けないときは、ログインの開始とコールバックの両方が属するパス配下にだけ絞る。
fn name_and_path(provider: Provider, secure: bool) -> (&'static str, &'static str) {
    match (provider, secure) {
        (Provider::Google, true) => ("__Host-oauth_state", "/"),
        (Provider::Google, false) => ("oauth_state", "/api/v1/auth/google"),
        (Provider::Line, true) => ("__Host-line_oauth_state", "/"),
        (Provider::Line, false) => ("line_oauth_state", "/api/v1/auth/line"),
        (Provider::Apple, true) => ("__Host-apple_oauth_state", "/"),
        (Provider::Apple, false) => ("apple_oauth_state", "/api/v1/auth/apple"),
    }
}

/// Apple は戻りが Apple のサイトからの POST (`form_post`) なので、`SameSite=Lax` では Cookie が
/// 届かない。Secure を付けられるときだけ `None` にする (Secure の無い `None` はブラウザが捨てる。
/// Apple は https の戻り先しか認めないので、Secure を付けない構成では Apple のログインは使えない)。
/// 届いた `state` は Cookie のものと照らし合わせ、期限・使い捨ては Google・LINE と同じ。
fn same_site(provider: Provider, secure: bool) -> SameSite {
    match (provider, secure) {
        (Provider::Apple, true) => SameSite::None,
        _ => SameSite::Lax,
    }
}
const COOKIE_TTL: Duration = Duration::seconds(600);

/// `state`・`code_verifier` などを1つの Cookie 値に詰める。どれも URL-safe base64 か英数字
/// (`.` を含まない) で生成されるため、区切り文字として安全に使える。
pub fn pack(values: &[&str]) -> String {
    values.join(".")
}

/// [`pack`] で詰めた値を `N` 個に戻す。個数が合わない壊れた値は `None` (検証失敗として扱われる)。
pub fn unpack<const N: usize>(value: &str) -> Option<[&str; N]> {
    value.split('.').collect::<Vec<_>>().try_into().ok()
}

fn build(provider: Provider, value: String, max_age: Duration, secure: bool) -> Cookie<'static> {
    let (name, path) = name_and_path(provider, secure);
    Cookie::build((name, value))
        .path(path)
        .http_only(true)
        .secure(secure)
        .same_site(same_site(provider, secure))
        .max_age(max_age)
        .build()
}

/// state Cookie を発行するための `Set-Cookie` ヘッダー値を作る。
pub fn set_header(provider: Provider, value: &str, secure: bool) -> HeaderValue {
    let cookie = build(provider, value.to_string(), COOKIE_TTL, secure);
    HeaderValue::from_str(&cookie.to_string()).expect("cookie header value should be valid")
}

/// state Cookie を破棄するための `Set-Cookie` ヘッダー値を作る。コールバック処理の成否に
/// 関わらず、使い捨てとして必ずこれで上書きする。
pub fn clear_header(provider: Provider, secure: bool) -> HeaderValue {
    let cookie = build(provider, String::new(), Duration::ZERO, secure);
    HeaderValue::from_str(&cookie.to_string()).expect("cookie header value should be valid")
}

/// リクエストの `Cookie` ヘッダーから state Cookie の値を取り出す。
pub fn read(headers: &HeaderMap, provider: Provider, secure: bool) -> Option<String> {
    let (name, _) = name_and_path(provider, secure);
    let raw = headers.get(header::COOKIE)?.to_str().ok()?;
    Cookie::split_parse(raw)
        .filter_map(Result::ok)
        .find(|c| c.name() == name)
        .map(|c| c.value().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::HeaderValue;

    #[test]
    fn pack_and_unpack_roundtrip() {
        let packed = pack(&["state-value", "verifier-value"]);
        assert_eq!(unpack(&packed), Some(["state-value", "verifier-value"]));
        let packed = pack(&["state", "verifier", "nonce"]);
        assert_eq!(unpack(&packed), Some(["state", "verifier", "nonce"]));
    }

    #[test]
    fn unpack_rejects_a_different_number_of_values() {
        assert_eq!(unpack::<2>("no-separator-here"), None);
        assert_eq!(unpack::<2>("a.b.c"), None);
        assert_eq!(unpack::<3>("a.b"), None);
    }

    /// Secure の有無で名前が変わっても、発行した Cookie を読み戻せる。
    #[test]
    fn set_header_roundtrips_through_read() {
        for provider in [Provider::Google, Provider::Line, Provider::Apple] {
            for secure in [false, true] {
                assert_eq!(
                    read(
                        &cookie_headers_from(&set_header(provider, "state.verifier", secure)),
                        provider,
                        secure
                    )
                    .as_deref(),
                    Some("state.verifier")
                );
            }
        }
    }

    /// Secure を付けないときの名前の Cookie は、Secure を付けるときには読まない (逆も同じ)。
    #[test]
    fn read_ignores_the_cookie_of_the_other_name() {
        let google = Provider::Google;
        assert_eq!(
            read(
                &cookie_headers_from(&set_header(google, "v.v", false)),
                google,
                true
            ),
            None
        );
        assert_eq!(
            read(
                &cookie_headers_from(&set_header(google, "v.v", true)),
                google,
                false
            ),
            None
        );
    }

    /// Apple の Cookie は、Apple のサイトからの POST でも届くよう `SameSite=None` にする。
    #[test]
    fn secure_apple_cookie_is_sent_on_cross_site_posts() {
        let header = set_header(Provider::Apple, "value", true);
        let text = header.to_str().expect("header value should be valid UTF-8");
        assert!(text.starts_with("__Host-apple_oauth_state="));
        assert!(text.contains("SameSite=None"));
        assert!(text.contains("Secure"));
    }

    /// Google と LINE を並行して始めても、互いの Cookie を読まない。
    #[test]
    fn read_ignores_the_cookie_of_the_other_provider() {
        for secure in [false, true] {
            let headers = cookie_headers_from(&set_header(Provider::Google, "v.v", secure));
            assert_eq!(read(&headers, Provider::Line, secure), None);
        }
    }

    /// `Set-Cookie` の値から、ブラウザが送り返す `Cookie` ヘッダーを作る。
    fn cookie_headers_from(header: &HeaderValue) -> HeaderMap {
        let mut headers = HeaderMap::new();
        // ブラウザが送り返す `Cookie` ヘッダーの形 (`name=value` のみ、属性は含まない) を模す。
        let cookie_pair = header
            .to_str()
            .expect("header value should be valid UTF-8")
            .split(';')
            .next()
            .expect("split should yield at least one part");
        headers.insert(
            header::COOKIE,
            HeaderValue::from_str(cookie_pair).expect("cookie pair should be a valid header value"),
        );

        headers
    }

    #[test]
    fn read_returns_none_when_cookie_header_missing() {
        let headers = HeaderMap::new();
        assert_eq!(read(&headers, Provider::Google, false), None);
    }

    #[test]
    fn set_header_has_expected_attributes() {
        let header = set_header(Provider::Google, "value", false);
        let text = header.to_str().expect("header value should be valid UTF-8");
        assert!(text.starts_with("oauth_state="));
        assert!(text.contains("HttpOnly"));
        assert!(text.contains("SameSite=Lax"));
        assert!(!text.contains("Secure"));
        assert!(text.contains("Path=/api/v1/auth/google"));

        let header = set_header(Provider::Line, "value", false);
        let text = header.to_str().expect("header value should be valid UTF-8");
        assert!(text.starts_with("line_oauth_state="));
        assert!(text.contains("Path=/api/v1/auth/line"));
    }

    /// `__Host-` の条件 (Secure・`Path=/`・`Domain` 無し) を満たす。満たさないとブラウザが Cookie を捨てる。
    #[test]
    fn secure_set_header_meets_host_prefix_requirements() {
        let header = set_header(Provider::Google, "value", true);
        let text = header.to_str().expect("header value should be valid UTF-8");
        assert!(text.starts_with("__Host-oauth_state="));
        assert!(text.contains("Secure"));
        assert!(text.contains("Path=/;") || text.ends_with("Path=/"));
        assert!(!text.contains("Domain"));
    }

    #[test]
    fn clear_header_expires_immediately() {
        let header = clear_header(Provider::Google, false);
        let text = header.to_str().expect("header value should be valid UTF-8");
        assert!(text.contains("Max-Age=0"));
    }
}
