//! リバースプロキシが付与する `X-Forwarded-*` (または Fly.io の `Fly-*`) の解釈。
//!
//! これらのヘッダーはクライアントが自由に名乗れるため、`[server] trusted_proxies` に挙げた
//! アドレス・範囲から接続された場合に限って採用する。設定が空 (既定) なら一切見ない。
//! 採用できたリクエストにだけ [`Forwarded`] が extensions に入るので、利用側は
//! 「入っていれば信頼できる値」として扱える。
//!
//! どのヘッダーを読むかは `[server] proxy_headers` で選ぶ ([`ProxyHeaders`])。

use std::net::{IpAddr, Ipv6Addr, SocketAddr};
use std::sync::Arc;

use axum::extract::{ConnectInfo, Request, State};
use axum::http::{Extensions, HeaderMap, Uri, header};
use axum::middleware::Next;
use axum::response::Response;
use serde::Deserialize;

const X_FORWARDED_PROTO: &str = "x-forwarded-proto";
const X_FORWARDED_HOST: &str = "x-forwarded-host";
const X_FORWARDED_FOR: &str = "x-forwarded-for";
const FLY_FORWARDED_PROTO: &str = "fly-forwarded-proto";
const FLY_CLIENT_IP: &str = "fly-client-ip";

/// 信頼済みプロキシから来たリクエストで、どのヘッダーを読むか。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ProxyHeaders {
    /// `X-Forwarded-Proto`/`-Host`/`-For`。受け取った値を破棄して自分の値を付け直すプロキシ向け。
    #[default]
    XForwarded,
    /// Fly.io のプロキシ (fly-proxy) が付ける `Fly-Forwarded-Proto` と `Fly-Client-IP`。
    ///
    /// ADR: fly-proxy はクライアントが送った `X-Forwarded-Proto`/`-Host` を書き換えずに渡し、
    /// `X-Forwarded-For` の末尾には app 自身のアドレスを足す (2026-09-17 に実測)。`Fly-*` の2つは
    /// fly-proxy が自分で付けるので、こちらだけを読む。`Host` はそのまま渡るので、転送先のホストは読まない。
    Fly,
}

/// 信頼済みプロキシ経由と判断できたリクエストの、プロキシが受け取った時点の情報。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Forwarded {
    /// `X-Forwarded-Proto` (Fly.io では `Fly-Forwarded-Proto`)。`http` / `https` 以外は解釈できないものとして捨てる。
    pub proto: Option<String>,
    /// `X-Forwarded-Host` の authority (host[:port])。Fly.io では読まないので常に `None`。
    pub host: Option<String>,
    /// `X-Forwarded-For` (Fly.io では `Fly-Client-IP`) から求めた、最も手前のクライアントのアドレス。
    pub client_ip: Option<IpAddr>,
}

impl Forwarded {
    /// 信頼できない場合 (設定が空・接続元が信頼リストに無い・接続元が分からない) は `None`。
    fn parse(
        trusted_proxies: &[TrustedProxy],
        proxy_headers: ProxyHeaders,
        peer: Option<IpAddr>,
        headers: &HeaderMap,
    ) -> Option<Self> {
        if trusted_proxies.is_empty() {
            return None;
        }
        let peer = peer?;
        if !is_trusted(trusted_proxies, peer) {
            return None;
        }

        Some(match proxy_headers {
            ProxyHeaders::XForwarded => Self {
                proto: proto(headers, X_FORWARDED_PROTO),
                host: single_value(headers, X_FORWARDED_HOST)
                    .map(|value| value.to_ascii_lowercase()),
                client_ip: client_ip(trusted_proxies, headers),
            },
            ProxyHeaders::Fly => Self {
                proto: proto(headers, FLY_FORWARDED_PROTO),
                host: None,
                client_ip: single_value(headers, FLY_CLIENT_IP)
                    .and_then(|value| parse_addr(&value)),
            },
        })
    }
}

/// `http` / `https` 以外は解釈できないものとして捨てる。
fn proto(headers: &HeaderMap, name: &str) -> Option<String> {
    single_value(headers, name)
        .map(|value| value.to_ascii_lowercase())
        .filter(|value| value == "http" || value == "https")
}

/// リクエストの接続元アドレス。`ConnectInfo` を付けずに `Router` を起動している場合
/// (統合テストの `oneshot` 等) は取得できない。
fn peer_ip(extensions: &Extensions) -> Option<IpAddr> {
    extensions
        .get::<ConnectInfo<SocketAddr>>()
        .map(|ConnectInfo(addr)| addr.ip().to_canonical())
}

fn is_trusted(trusted_proxies: &[TrustedProxy], addr: IpAddr) -> bool {
    trusted_proxies.iter().any(|trusted| trusted.contains(addr))
}

/// `[server] trusted_proxies` の1要素。アドレス (`127.0.0.1`) か、CIDR 表記の範囲 (`172.18.0.0/16`)。
///
/// 範囲は、Docker のネットワークのように、プロキシのアドレスがコンテナを作り直すたびに変わる構成向け。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TrustedProxy {
    network: IpAddr,
    prefix_len: u8,
}

impl TrustedProxy {
    /// IPv4 射影アドレス (`::ffff:127.0.0.1`) で来ても IPv4 の設定と一致させるため、canonical form で比べる。
    pub(crate) fn contains(self, addr: IpAddr) -> bool {
        let ((network, width), (addr, _)) = match (self.network, addr.to_canonical()) {
            (network @ IpAddr::V4(_), addr @ IpAddr::V4(_))
            | (network @ IpAddr::V6(_), addr @ IpAddr::V6(_)) => (bits(network), bits(addr)),
            _ => return false,
        };
        network_part(network, width, self.prefix_len) == network_part(addr, width, self.prefix_len)
    }
}

/// アドレスのビット列と、そのビット幅。
fn bits(addr: IpAddr) -> (u128, u32) {
    match addr {
        IpAddr::V4(v4) => (u128::from(v4.to_bits()), 32),
        IpAddr::V6(v6) => (v6.to_bits(), 128),
    }
}

/// 先頭 `prefix_len` ビットだけを残した値 (右に詰める)。
fn network_part(bits: u128, width: u32, prefix_len: u8) -> u128 {
    bits.checked_shr(width - u32::from(prefix_len)).unwrap_or(0)
}

impl std::str::FromStr for TrustedProxy {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let invalid = || {
            format!(
                "trusted_proxies の要素をアドレスか CIDR (例: 172.18.0.0/16) として解釈できません ({value})"
            )
        };
        let Some((network, prefix_len)) = value.split_once('/') else {
            let network = value
                .parse::<IpAddr>()
                .map_err(|_| invalid())?
                .to_canonical();
            let (_, width) = bits(network);
            return Ok(Self {
                network,
                prefix_len: width as u8,
            });
        };

        let network = network.parse::<IpAddr>().map_err(|_| invalid())?;
        // 接続元は canonical form (IPv4) で比べるため、IPv4 射影アドレスで書いた範囲はどれにも一致しない。
        if matches!(network, IpAddr::V6(v6) if v6.to_ipv4_mapped().is_some()) {
            return Err(format!(
                "trusted_proxies の IPv4 の範囲は IPv4 の表記で書いてください ({value})"
            ));
        }
        let (network_bits, width) = bits(network);
        let prefix_len = prefix_len
            .parse::<u8>()
            .ok()
            .filter(|len| u32::from(*len) <= width)
            .ok_or_else(invalid)?;
        // ADR: ホスト部にビットが立っている範囲 (`172.18.0.3/16`) は弾く。1台のつもりのアドレスに
        // `/16` を付けたような書き間違いで、意図より広く信頼してしまわないようにするため。
        let host_bits = width - u32::from(prefix_len);
        let masked = network_part(network_bits, width, prefix_len)
            .checked_shl(host_bits)
            .unwrap_or(0);
        if masked != network_bits {
            return Err(format!(
                "trusted_proxies の範囲は、ホスト部を 0 にしたネットワークアドレスで書いてください ({value})"
            ));
        }
        Ok(Self {
            network,
            prefix_len,
        })
    }
}

impl<'de> Deserialize<'de> for TrustedProxy {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        String::deserialize(deserializer)?
            .parse()
            .map_err(serde::de::Error::custom)
    }
}

/// 値がちょうど1つのときだけ、その値を返す。
///
/// `X-Forwarded-Proto`/`-Host` は、プロキシが受信した値を破棄して自分の値だけを設定する
/// のが正しい振る舞い。複数行・カンマ区切りで届くのは、クライアントが名乗った値をプロキシが
/// 消さずに追記している構成であり、どれがプロキシ由来かを区別できない。先頭を採ると
/// クライアントが好きな値を通せてしまうため、複数あれば値なしとして扱う。
fn single_value(headers: &HeaderMap, name: &str) -> Option<String> {
    let mut values = headers
        .get_all(name)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .flat_map(|value| value.split(','))
        .map(str::trim)
        .filter(|value| !value.is_empty());

    let value = values.next()?;
    if values.next().is_some() {
        return None;
    }
    Some(value.to_string())
}

/// `X-Forwarded-For` の並びから、クライアントのアドレスを求める。
///
/// このヘッダーは各プロキシが追記していくのが仕様どおりの振る舞いなので、複数の値を許す。
/// 末尾から順に、信頼済みプロキシのアドレスである限り読み飛ばし、最初に現れた
/// 「信頼していないアドレス」をクライアントとする。詐称された値は信頼済みプロキシが追記した
/// 実アドレスより左に積まれるため、この読み方であれば混入しない。
/// すべてが信頼済みプロキシのアドレスだった場合はクライアントを特定できないので `None`
/// (プロキシのアドレスをクライアントとして返すと、レートリミット等で全員が同一視される)。
fn client_ip(trusted_proxies: &[TrustedProxy], headers: &HeaderMap) -> Option<IpAddr> {
    let chain: Vec<IpAddr> = headers
        .get_all(X_FORWARDED_FOR)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .flat_map(|value| value.split(','))
        .filter_map(parse_addr)
        .collect();

    chain
        .iter()
        .rev()
        .find(|addr| !is_trusted(trusted_proxies, **addr))
        .copied()
}

/// `X-Forwarded-For` の要素をアドレスとして読む。ポート付き (`1.2.3.4:5678`・
/// `[::1]:5678`) で書くプロキシがあるため、素の IP として読めなければ
/// ソケットアドレスとしても試す。
fn parse_addr(value: &str) -> Option<IpAddr> {
    let value = value.trim();
    value
        .parse::<IpAddr>()
        .ok()
        .or_else(|| value.parse::<SocketAddr>().ok().map(|addr| addr.ip()))
        .map(|addr| addr.to_canonical())
}

/// レートリミットに使う、クライアントのアドレス。信頼済みプロキシ経由ならプロキシが伝えた
/// アドレス、そうでなければ接続元のアドレス。
///
/// 求められない場合 (プロキシ経由だがクライアントを特定できない・`ConnectInfo` が無い) は
/// `None`。プロキシ自身のアドレスで代用すると、全員が同一のクライアントとして数えられるため。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClientIp(pub Option<IpAddr>);

impl<S: Send + Sync> axum::extract::FromRequestParts<S> for ClientIp {
    type Rejection = std::convert::Infallible;

    async fn from_request_parts(
        parts: &mut axum::http::request::Parts,
        _state: &S,
    ) -> Result<Self, Self::Rejection> {
        let ip = match parts.extensions.get::<Forwarded>() {
            Some(forwarded) => forwarded.client_ip,
            None => peer_ip(&parts.extensions),
        };
        Ok(Self(ip))
    }
}

/// レートリミットで接続元を数える単位のキー。IPv4 はアドレスごと、IPv6 は先頭 /64 ごと。
///
/// IPv6 は1台・1契約に /64 が割り当てられるのが普通で、アドレスごとに数えると送信元を
/// 変えるだけで接続元単位の制限を外れられるため (docs/authentication.md)。
pub fn rate_limit_key(ip: IpAddr) -> String {
    match ip.to_canonical() {
        IpAddr::V4(v4) => v4.to_string(),
        IpAddr::V6(v6) => {
            let prefix = u128::from(v6) & !((1u128 << 64) - 1);
            format!("{}/64", Ipv6Addr::from(prefix))
        }
    }
}

/// [`rate_limit_key`] の、接続元が分からない要求をまとめて1つの接続元として数える版
/// (メールを送らせる要求・リンクの先でのパスワードの設定)。数えずに通すと、制限を外れる経路になるため。
pub fn rate_limit_key_or_shared(client_ip: Option<IpAddr>) -> String {
    client_ip.map(rate_limit_key).unwrap_or_default()
}

/// リクエスト自身の authority (host[:port]、小文字)。HTTP/2 では `Host` が省略され得るため、
/// 無ければ `:authority` 由来の `uri.authority()` を使う。
pub fn request_authority(headers: &HeaderMap, uri: &Uri) -> Option<String> {
    headers
        .get(header::HOST)
        .and_then(|value| value.to_str().ok())
        .map(str::to_string)
        .or_else(|| uri.authority().map(|a| a.as_str().to_string()))
        .map(|host| host.to_ascii_lowercase())
}

/// `middleware::from_fn_with_state` に渡す、信頼するプロキシと読むヘッダー。
#[derive(Debug, Clone)]
pub struct ProxySettings {
    pub trusted_proxies: Arc<[TrustedProxy]>,
    pub proxy_headers: ProxyHeaders,
}

/// プロキシが付けたヘッダーを解釈して extensions に入れるミドルウェア。
/// CSRF チェックがこの結果を使うため、それより外側に積む (`src/lib.rs`)。
pub async fn extract(
    State(settings): State<ProxySettings>,
    mut req: Request,
    next: Next,
) -> Response {
    if let Some(forwarded) = Forwarded::parse(
        &settings.trusted_proxies,
        settings.proxy_headers,
        peer_ip(req.extensions()),
        req.headers(),
    ) {
        req.extensions_mut().insert(forwarded);
    }
    next.run(req).await
}

#[cfg(test)]
mod tests {
    use super::*;

    fn headers(pairs: &[(&str, &str)]) -> HeaderMap {
        let mut headers = HeaderMap::new();
        for (name, value) in pairs {
            headers.append(
                axum::http::HeaderName::from_bytes(name.as_bytes()).expect("ヘッダー名が不正"),
                value.parse().expect("ヘッダー値が不正"),
            );
        }
        headers
    }

    fn ip(value: &str) -> IpAddr {
        value.parse().expect("アドレスが不正")
    }

    fn proxy(value: &str) -> TrustedProxy {
        value.parse().expect("信頼するプロキシの書き方が不正")
    }

    #[test]
    fn trusted_proxy_address_matches_only_itself() {
        let trusted = proxy("172.18.0.3");
        assert!(trusted.contains(ip("172.18.0.3")));
        assert!(trusted.contains(ip("::ffff:172.18.0.3")));
        assert!(!trusted.contains(ip("172.18.0.4")));
    }

    #[test]
    fn trusted_proxy_range_matches_addresses_in_the_prefix() {
        let v4 = proxy("172.18.0.0/16");
        assert!(v4.contains(ip("172.18.0.3")));
        assert!(v4.contains(ip("172.18.255.255")));
        assert!(v4.contains(ip("::ffff:172.18.9.9")));
        assert!(!v4.contains(ip("172.19.0.1")));
        assert!(!v4.contains(ip("2001:db8::1")));

        let v6 = proxy("fd00:1::/64");
        assert!(v6.contains(ip("fd00:1::abcd")));
        assert!(!v6.contains(ip("fd00:2::1")));
        assert!(!v6.contains(ip("172.18.0.3")));

        assert!(proxy("0.0.0.0/0").contains(ip("203.0.113.7")));
        assert!(proxy("::/0").contains(ip("2001:db8::1")));

        // プレフィックスが全ビットのとき (シフト量 0) は、そのアドレスだけに一致する。
        let host_v4 = proxy("172.18.0.3/32");
        assert!(host_v4.contains(ip("172.18.0.3")));
        assert!(!host_v4.contains(ip("172.18.0.2")));
        assert!(!host_v4.contains(ip("172.18.0.4")));
        let host_v6 = proxy("fd00:1::1/128");
        assert!(host_v6.contains(ip("fd00:1::1")));
        assert!(!host_v6.contains(ip("fd00:1::")));
        assert!(!host_v6.contains(ip("fd00:1::2")));
    }

    #[test]
    fn trusted_proxy_rejects_malformed_values() {
        for value in [
            "proxy",
            "172.18.0.0/33",
            "fd00::/129",
            "172.18.0.0/",
            "172.18.0.0/-1",
            "172.18.0.3/16",
            "fd00:1::1/64",
            "::ffff:172.18.0.0/112",
        ] {
            assert!(value.parse::<TrustedProxy>().is_err(), "{value}");
        }
    }

    async fn client_ip_of(request: Request) -> ClientIp {
        let (mut parts, _) = request.into_parts();
        <ClientIp as axum::extract::FromRequestParts<()>>::from_request_parts(&mut parts, &())
            .await
            .expect("Infallible")
    }

    #[tokio::test]
    async fn client_ip_prefers_the_address_a_trusted_proxy_reported() {
        let mut request = Request::new(axum::body::Body::empty());
        request.extensions_mut().insert(ConnectInfo(
            "127.0.0.1:5000".parse::<SocketAddr>().expect("固定値"),
        ));
        request.extensions_mut().insert(Forwarded {
            proto: None,
            host: None,
            client_ip: Some(ip("203.0.113.7")),
        });
        assert_eq!(
            client_ip_of(request).await,
            ClientIp(Some(ip("203.0.113.7")))
        );
    }

    /// プロキシ経由だがクライアントを特定できないときに、プロキシのアドレスで代用しない。
    #[tokio::test]
    async fn client_ip_does_not_fall_back_to_the_proxy_address() {
        let mut request = Request::new(axum::body::Body::empty());
        request.extensions_mut().insert(ConnectInfo(
            "127.0.0.1:5000".parse::<SocketAddr>().expect("固定値"),
        ));
        request.extensions_mut().insert(Forwarded {
            proto: None,
            host: None,
            client_ip: None,
        });
        assert_eq!(client_ip_of(request).await, ClientIp(None));
    }

    #[tokio::test]
    async fn client_ip_uses_the_peer_without_a_trusted_proxy() {
        let mut request = Request::new(axum::body::Body::empty());
        request.extensions_mut().insert(ConnectInfo(
            "[::ffff:192.168.1.9]:5000"
                .parse::<SocketAddr>()
                .expect("固定値"),
        ));
        assert_eq!(
            client_ip_of(request).await,
            ClientIp(Some(ip("192.168.1.9")))
        );
    }

    #[test]
    fn parse_returns_none_when_no_trusted_proxy_is_configured() {
        let headers = headers(&[("x-forwarded-proto", "https")]);
        assert_eq!(
            Forwarded::parse(
                &[],
                ProxyHeaders::XForwarded,
                Some(ip("127.0.0.1")),
                &headers
            ),
            None
        );
    }

    #[test]
    fn parse_returns_none_when_peer_is_not_trusted() {
        let headers = headers(&[("x-forwarded-proto", "https")]);
        let trusted = [proxy("127.0.0.1")];
        assert_eq!(
            Forwarded::parse(
                &trusted,
                ProxyHeaders::XForwarded,
                Some(ip("192.168.1.9")),
                &headers
            ),
            None
        );
    }

    #[test]
    fn parse_returns_none_when_peer_is_unknown() {
        let headers = headers(&[("x-forwarded-proto", "https")]);
        let trusted = [proxy("127.0.0.1")];
        assert_eq!(
            Forwarded::parse(&trusted, ProxyHeaders::XForwarded, None, &headers),
            None
        );
    }

    #[test]
    fn parse_accepts_ipv4_mapped_peer() {
        let headers = headers(&[("x-forwarded-proto", "https")]);
        let trusted = [proxy("127.0.0.1")];
        let forwarded = Forwarded::parse(
            &trusted,
            ProxyHeaders::XForwarded,
            Some(ip("::ffff:127.0.0.1")),
            &headers,
        )
        .expect("信頼済みプロキシとして扱われるはず");
        assert_eq!(forwarded.proto.as_deref(), Some("https"));
    }

    #[test]
    fn parse_reads_proto_host_and_client_ip() {
        let headers = headers(&[
            ("x-forwarded-proto", "HTTPS"),
            ("x-forwarded-host", "BP.example.com"),
            ("x-forwarded-for", "203.0.113.7"),
        ]);
        let trusted = [proxy("127.0.0.1")];
        let forwarded = Forwarded::parse(
            &trusted,
            ProxyHeaders::XForwarded,
            Some(ip("127.0.0.1")),
            &headers,
        )
        .expect("信頼済みプロキシとして扱われるはず");
        assert_eq!(forwarded.proto.as_deref(), Some("https"));
        assert_eq!(forwarded.host.as_deref(), Some("bp.example.com"));
        assert_eq!(forwarded.client_ip, Some(ip("203.0.113.7")));
    }

    #[test]
    fn parse_discards_proto_and_host_appended_to_a_client_supplied_value() {
        // プロキシが受信値を破棄していない構成。先頭はクライアントが名乗った値になり得る。
        let headers = headers(&[
            ("x-forwarded-host", "evil.example, bp.example.com"),
            ("x-forwarded-proto", "https"),
            ("x-forwarded-proto", "https"),
        ]);
        let trusted = [proxy("127.0.0.1")];
        let forwarded = Forwarded::parse(
            &trusted,
            ProxyHeaders::XForwarded,
            Some(ip("127.0.0.1")),
            &headers,
        )
        .expect("信頼済みプロキシとして扱われるはず");
        assert_eq!(forwarded.host, None);
        assert_eq!(forwarded.proto, None);
    }

    /// Fly.io では、クライアントが名乗れる `X-Forwarded-*` を読まず、fly-proxy が付ける `Fly-*` だけを読む。
    #[test]
    fn fly_reads_only_the_headers_fly_proxy_sets() {
        let headers = headers(&[
            ("x-forwarded-proto", "http"),
            ("x-forwarded-host", "evil.example"),
            ("x-forwarded-for", "198.51.100.1, 203.0.113.7, 66.241.124.1"),
            ("fly-forwarded-proto", "https"),
            ("fly-client-ip", "203.0.113.7"),
        ]);
        let trusted = [proxy("172.16.0.0/12")];
        let forwarded = Forwarded::parse(
            &trusted,
            ProxyHeaders::Fly,
            Some(ip("172.16.5.10")),
            &headers,
        )
        .expect("信頼済みプロキシとして扱われるはず");
        assert_eq!(
            forwarded,
            Forwarded {
                proto: Some("https".to_string()),
                host: None,
                client_ip: Some(ip("203.0.113.7")),
            }
        );
    }

    #[test]
    fn fly_is_ignored_when_peer_is_not_trusted() {
        let headers = headers(&[("fly-client-ip", "203.0.113.7")]);
        let trusted = [proxy("172.16.0.0/12")];
        assert_eq!(
            Forwarded::parse(&trusted, ProxyHeaders::Fly, Some(ip("192.0.2.1")), &headers),
            None
        );
    }

    #[test]
    fn fly_discards_repeated_client_ip() {
        let headers = headers(&[("fly-client-ip", "198.51.100.1, 203.0.113.7")]);
        let trusted = [proxy("172.16.0.0/12")];
        let forwarded = Forwarded::parse(
            &trusted,
            ProxyHeaders::Fly,
            Some(ip("172.16.5.10")),
            &headers,
        )
        .expect("信頼済みプロキシとして扱われるはず");
        assert_eq!(forwarded.client_ip, None);
    }

    #[test]
    fn parse_discards_unknown_proto() {
        let headers = headers(&[("x-forwarded-proto", "ftp")]);
        let trusted = [proxy("127.0.0.1")];
        let forwarded = Forwarded::parse(
            &trusted,
            ProxyHeaders::XForwarded,
            Some(ip("127.0.0.1")),
            &headers,
        )
        .expect("信頼済みプロキシとして扱われるはず");
        assert_eq!(forwarded.proto, None);
    }

    #[test]
    fn client_ip_skips_trailing_trusted_proxies() {
        let headers = headers(&[("x-forwarded-for", "203.0.113.7, 10.0.0.2, 127.0.0.1")]);
        let trusted = [proxy("127.0.0.1"), proxy("10.0.0.2")];
        assert_eq!(client_ip(&trusted, &headers), Some(ip("203.0.113.7")));
    }

    #[test]
    fn client_ip_ignores_spoofed_value_before_the_real_one() {
        // クライアントが自分で付けた値 (先頭) は、プロキシが追記した実アドレスより左に積まれる。
        let headers = headers(&[("x-forwarded-for", "10.0.0.2, 203.0.113.7")]);
        let trusted = [proxy("127.0.0.1"), proxy("10.0.0.2")];
        assert_eq!(client_ip(&trusted, &headers), Some(ip("203.0.113.7")));
    }

    #[test]
    fn client_ip_is_none_when_every_entry_is_trusted() {
        let headers = headers(&[("x-forwarded-for", "127.0.0.1, 127.0.0.1")]);
        let trusted = [proxy("127.0.0.1")];
        assert_eq!(client_ip(&trusted, &headers), None);
    }

    #[test]
    fn client_ip_reads_entries_with_port() {
        let headers = headers(&[("x-forwarded-for", "[2001:db8::1]:4711, 127.0.0.1")]);
        let trusted = [proxy("127.0.0.1")];
        assert_eq!(client_ip(&trusted, &headers), Some(ip("2001:db8::1")));
    }

    #[test]
    fn client_ip_is_none_without_the_header() {
        assert_eq!(client_ip(&[proxy("127.0.0.1")], &HeaderMap::new()), None);
    }

    #[test]
    fn rate_limit_key_groups_ipv6_addresses_in_the_same_64_prefix() {
        assert_eq!(rate_limit_key(ip("2001:db8:1:2::1")), "2001:db8:1:2::/64");
        assert_eq!(
            rate_limit_key(ip("2001:db8:1:2::1")),
            rate_limit_key(ip("2001:db8:1:2:ffff:ffff:ffff:ffff"))
        );
        assert_ne!(
            rate_limit_key(ip("2001:db8:1:2::1")),
            rate_limit_key(ip("2001:db8:1:3::1"))
        );
    }

    #[test]
    fn rate_limit_key_keeps_each_ipv4_address_separate() {
        assert_eq!(rate_limit_key(ip("192.0.2.10")), "192.0.2.10");
        assert_ne!(
            rate_limit_key(ip("192.0.2.10")),
            rate_limit_key(ip("192.0.2.11"))
        );
        // IPv4 射影アドレスで届いても、IPv4 として数える。
        assert_eq!(rate_limit_key(ip("::ffff:192.0.2.10")), "192.0.2.10");
    }
}
