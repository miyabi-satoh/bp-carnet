//! 端末の印の Cookie。ウェブで、新しい端末からのログインを見分けるために置く
//! (docs/authentication.md「ログインの記録と知らせ」)。
//!
//! 値はランダムで、DB にはハッシュだけを、ログインの成功の記録 (`auth_events.device_hash`) と一緒に残す。
//! ログインの証しではなく、本人への知らせを省くかにしか使わない。

use axum::http::{HeaderMap, HeaderValue, header};
use time::Duration;
use tower_sessions::cookie::{Cookie, SameSite};

use crate::token;

/// ブラウザが Cookie に認める期限の上限 (400日)。それより長く置くと、ブラウザが400日に縮める。
const MAX_AGE: Duration = Duration::days(400);

/// 値のバイト数。32 バイトは URL-safe base64 で43文字になる。
const VALUE_BYTES: usize = 32;
const VALUE_LEN: usize = 43;

/// Secure を付けるときは、セッション Cookie (`crate::session::cookie_name`) と同じ理由で `__Host-` を付ける。
fn name(secure: bool) -> &'static str {
    if secure { "__Host-device" } else { "device" }
}

/// ログインに成功したブラウザの印。
pub struct Device {
    value: String,
}

impl Device {
    /// リクエストに付いてきた印。無いか形が違えば、新しく作る。
    pub fn from_request(headers: &HeaderMap, secure: bool) -> Self {
        let value = read(headers, secure).unwrap_or_else(|| token::random_url_safe(VALUE_BYTES));
        Self { value }
    }

    /// DB に残すハッシュ。
    pub fn hash(&self) -> String {
        token::sha256_url_safe(&self.value)
    }

    /// 印を置く (置き直して期限を延ばす) `Set-Cookie` の値。
    ///
    /// Secure を付けられるときは `SameSite=None` にする。Apple の戻り (Apple のサイトからの POST) にも届かせるため。
    /// ほかのサイトからの送信に付いても、知らせを省くかにしか使わないので害が無い。
    /// Secure の無い `None` はブラウザが捨てるので、そのときは `Lax` にする。
    pub fn set_header(&self, secure: bool) -> HeaderValue {
        let cookie = Cookie::build((name(secure), self.value.clone()))
            .path("/")
            .http_only(true)
            .secure(secure)
            .same_site(if secure {
                SameSite::None
            } else {
                SameSite::Lax
            })
            .max_age(MAX_AGE)
            .build();
        crate::session::set_cookie_header(&cookie)
    }
}

/// 自分が作った形 (URL-safe base64 の43文字) の値だけを読む。ほかの値は、無いものとして扱う。
fn read(headers: &HeaderMap, secure: bool) -> Option<String> {
    let name = name(secure);
    headers
        .get_all(header::COOKIE)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .flat_map(Cookie::split_parse)
        .filter_map(Result::ok)
        .find(|cookie| cookie.name() == name)
        .map(|cookie| cookie.value().to_string())
        .filter(|value| {
            value.len() == VALUE_LEN
                && value
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request_with(cookie: &str) -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert(
            header::COOKIE,
            HeaderValue::from_str(cookie).expect("固定の値"),
        );
        headers
    }

    /// 置いた印を、次のリクエストで同じ印として読み戻せる。
    #[test]
    fn a_device_set_by_the_server_is_read_back() {
        for secure in [false, true] {
            let first = Device::from_request(&HeaderMap::new(), secure);
            let set_cookie = first.set_header(secure);
            let pair = set_cookie
                .to_str()
                .expect("ASCII のはず")
                .split(';')
                .next()
                .expect("名前と値がある")
                .to_string();
            let second = Device::from_request(&request_with(&pair), secure);
            assert_eq!(first.hash(), second.hash(), "secure={secure}");
        }
    }

    #[test]
    fn a_malformed_value_is_replaced_with_a_new_device() {
        let forged = Device::from_request(&request_with("device=short"), false);
        assert_ne!(forged.hash(), token::sha256_url_safe("short"));
        assert_eq!(forged.value.len(), VALUE_LEN);
    }

    #[test]
    fn the_cookie_uses_the_host_prefix_and_same_site_none_when_secure() {
        let device = Device::from_request(&HeaderMap::new(), true);
        let header = device.set_header(true);
        let header = header.to_str().expect("ASCII のはず");
        assert!(header.starts_with("__Host-device="), "{header}");
        assert!(header.contains("SameSite=None"), "{header}");
        assert!(header.contains("Secure"), "{header}");
        assert!(header.contains("HttpOnly"), "{header}");
        assert!(header.contains("Max-Age=34560000"), "{header}");
    }
}
