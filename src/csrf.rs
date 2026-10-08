//! GET 以外のリクエストに対する同一オリジンチェック (CSRF 対策)。
//!
//! ブラウザはクロスサイトの状態変更リクエスト (フォーム送信・fetch 等) で `Origin` ヘッダーを
//! 送る (`Origin` を送らない古い実装のためのフォールバックとして `Referer` も見る)。この
//! authority (host[:port]) がリクエスト自身の authority と一致しなければ、別ホストのページから
//! 送られたリクエストとして拒否する。`Origin`/`Referer` がどちらも無いリクエストは許可する:
//! ブラウザが送るクロスサイトの状態変更リクエストは通常どちらかを伴うため、これらを送らない
//! 非ブラウザクライアント (API クライアント・ヘルスチェック等) との互換性を優先する。
//!
//! リクエスト自身の authority は、信頼済みプロキシが伝えた host (`X-Forwarded-Host`) があればそれを、
//! 無ければ `Host` ヘッダー (HTTP/2 では `Host` が省略され得るため `:authority` 由来の
//! `req.uri().authority()`) を使う。
//!
//! scheme (http/https) は、信頼済みプロキシが伝えた proto が得られたときだけ比較する
//! (`src/forwarded.rs`。読むヘッダーは `[server] proxy_headers` で決まる)。それが無ければ
//! backend 自身は実際の接続が https 経由かを知る手段が無く、比較しても検証にならないため
//! authority だけを見る。この場合、同一ホストの
//! http/https 間でのリクエスト (プロキシ設定の不備等で http 版が到達可能な場合) はすり抜け得る。
//!
//! 「GET 以外」は文字列どおりに解釈し、HEAD/OPTIONS も対象にする (安全側の判断であり見落としでは
//! ない)。モバイルアプリの preflight (OPTIONS) は、外側の CORS レイヤーがここへ来る前に答える。
//!
//! モバイルアプリ (`Origin: capacitor://localhost`、`crate::app_token::APP_ORIGIN`) からのリクエストは
//! 通す (docs/mobile-app.md)。アプリの WebView はサイトが違うため bp.amiiby.com の Cookie を送らず、
//! ログインの証しはアプリが自分で付けるトークンだけなので、なりすましの操作に使われる Cookie が無い。
//!
//! Apple のログインのコールバック (`crate::apple_login::CALLBACK_PATH`) も通す。Apple のサイトからの
//! POST (`form_post`) で届くためで、`state` の照合で守る (docs/authentication.md)。
//!
//! `tower-sessions` のデフォルト `SameSite=Strict` (`src/session.rs`) により古典的な
//! クロスサイトフォーム POST は既に大きく緩和されているが、これは「同一サイト (site)」単位の
//! 緩い制限であり「同一ホスト」の検証ではないため、本チェックで補う。

use axum::extract::Request;
use axum::http::{Method, Uri, header};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};

use crate::error::AppError;
use crate::forwarded::{self, Forwarded};

pub async fn same_origin_check(req: Request, next: Next) -> Response {
    if req.method() == Method::GET {
        return next.run(req).await;
    }

    let source = req
        .headers()
        .get(header::ORIGIN)
        .or_else(|| req.headers().get(header::REFERER));
    let Some(source) = source else {
        // Origin/Referer 無しは許可する (上記モジュールコメント参照)。
        return next.run(req).await;
    };
    if source.as_bytes() == crate::app_token::APP_ORIGIN.as_bytes() {
        return next.run(req).await;
    }
    // Apple のコールバックは、Apple のサイトからの POST (`form_post`)。`state` を専用 Cookie
    // (`crate::oauth_cookie`) のものと照らし合わせて守る (docs/authentication.md)。
    if req.method() == Method::POST && req.uri().path() == crate::apple_login::CALLBACK_PATH {
        return next.run(req).await;
    }
    // 値はあるが読めない場合 (`Origin: null` 等) は、無い場合とは区別して不一致として扱う。
    let source = source.to_str().ok().map(str::to_string);

    let forwarded = req.extensions().get::<Forwarded>();
    let expected_host = forwarded
        .and_then(|forwarded| forwarded.host.clone())
        .or_else(|| host_of(&req));
    let expected_scheme = forwarded.and_then(|forwarded| forwarded.proto.clone());
    let client_ip = forwarded.and_then(|forwarded| forwarded.client_ip);

    if is_same_origin(
        source.as_deref(),
        expected_host.as_deref(),
        expected_scheme.as_deref(),
    ) {
        next.run(req).await
    } else {
        // リバースプロキシが Host を書き換えている等、設定ミスだと全 POST/PUT/DELETE が
        // 無言で 403 になり気づきにくいため、warn で残す (Cookie 等の機微情報は含めない)。
        tracing::warn!(
            method = %req.method(),
            path = %req.uri().path(),
            host = ?expected_host,
            scheme = ?expected_scheme,
            client_ip = ?client_ip,
            source = ?source,
            "CSRF: 同一オリジンチェック不一致によりリクエストを拒否",
        );
        AppError::CsrfRejected.into_response()
    }
}

/// リクエスト自身の authority (host[:port])。
fn host_of(req: &Request) -> Option<String> {
    forwarded::request_authority(req.headers(), req.uri())
}

/// `Origin`/`Referer` の値が、リクエスト自身のオリジンと同じか。
/// `scheme` が `None` (信頼済みプロキシが伝えた proto が無い) の場合は authority だけで判断する。
fn is_same_origin(source: Option<&str>, host: Option<&str>, scheme: Option<&str>) -> bool {
    let (Some(source), Some(host)) = (source, host) else {
        return false;
    };
    let Ok(uri) = source.parse::<Uri>() else {
        return false;
    };
    let Some(authority) = uri.authority() else {
        return false;
    };
    if authority.as_str().to_ascii_lowercase() != host {
        return false;
    }
    match scheme {
        Some(expected) => uri
            .scheme_str()
            .is_some_and(|actual| actual.eq_ignore_ascii_case(expected)),
        None => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_when_authority_is_equal() {
        assert!(is_same_origin(
            Some("http://example.com:3000"),
            Some("example.com:3000"),
            None
        ));
    }

    #[test]
    fn does_not_match_different_authority() {
        assert!(!is_same_origin(
            Some("http://evil.example"),
            Some("example.com"),
            None
        ));
    }

    #[test]
    fn ignores_case_of_the_source_authority() {
        assert!(is_same_origin(
            Some("http://EXAMPLE.com"),
            Some("example.com"),
            None
        ));
    }

    #[test]
    fn does_not_match_unparsable_source() {
        // `Origin: null` (サンドボックス化された iframe 等) は authority が取れない。
        assert!(!is_same_origin(Some("null"), Some("example.com"), None));
    }

    #[test]
    fn does_not_match_without_a_host() {
        assert!(!is_same_origin(Some("http://example.com"), None, None));
    }

    #[test]
    fn matches_when_scheme_is_equal() {
        assert!(is_same_origin(
            Some("https://example.com"),
            Some("example.com"),
            Some("https")
        ));
    }

    #[test]
    fn does_not_match_different_scheme() {
        assert!(!is_same_origin(
            Some("http://example.com"),
            Some("example.com"),
            Some("https")
        ));
    }
}
