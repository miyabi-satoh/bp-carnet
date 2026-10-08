//! モバイルアプリの版の確認 (docs/mobile-app.md)。
//!
//! アプリは利用者が更新するまで古い版のまま残る。API を変えて古い版と合わなくなったら、
//! `[mobile_app] min_version` を上げて、それより古い版の API の呼び出しを断る。アプリは断りを受けて
//! 更新を促す画面を出す。

use std::cmp::Ordering;

use axum::extract::{Request, State};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use serde::Deserialize;

use crate::app_token;
use crate::error::AppError;

/// アプリが自分の版 (`CFBundleShortVersionString`) を載せて送るヘッダー。
pub const HEADER: &str = "x-app-version";

/// 数字をドットでつないだ版 (`1.2.3`)。欠けた桁は 0 として比べる (`1.2` と `1.2.0` は同じ)。
#[derive(Debug, Clone)]
pub struct AppVersion(Vec<u32>);

impl std::str::FromStr for AppVersion {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        value
            .split('.')
            .map(|part| {
                // `u32::from_str` は `+1` を通すので、数字だけかを先に見る。
                if part.is_empty() || !part.bytes().all(|b| b.is_ascii_digit()) {
                    return Err(format!(
                        "版は数字をドットでつないだ形にしてください ({value})"
                    ));
                }
                part.parse()
                    .map_err(|_| format!("版の数字が大きすぎます ({value})"))
            })
            .collect::<Result<_, _>>()
            .map(Self)
    }
}

impl<'de> Deserialize<'de> for AppVersion {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        String::deserialize(deserializer)?
            .parse()
            .map_err(serde::de::Error::custom)
    }
}

impl Ord for AppVersion {
    fn cmp(&self, other: &Self) -> Ordering {
        let len = self.0.len().max(other.0.len());
        (0..len)
            .map(|i| {
                let a = self.0.get(i).copied().unwrap_or(0);
                let b = other.0.get(i).copied().unwrap_or(0);
                a.cmp(&b)
            })
            .find(|ordering| ordering.is_ne())
            .unwrap_or(Ordering::Equal)
    }
}

impl PartialEq for AppVersion {
    fn eq(&self, other: &Self) -> bool {
        self.cmp(other).is_eq()
    }
}

impl Eq for AppVersion {}

impl PartialOrd for AppVersion {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

/// アプリの画面からの API の呼び出しで、版が `min_version` より古ければ断る。
///
/// - 版を送らない・読めない呼び出しも古いものとして断る (公開した版はどれも送るため、送らないのは
///   この仕組みより前の開発中の版だけ)。
/// - ウェブ版 (出どころがアプリでない) は見ない。
pub async fn require_supported(
    State(min_version): State<Option<AppVersion>>,
    req: Request,
    next: Next,
) -> Response {
    let Some(min_version) = min_version else {
        return next.run(req).await;
    };
    if !req.uri().path().starts_with("/api/") || !app_token::is_from_app(req.headers()) {
        return next.run(req).await;
    }
    let version = req
        .headers()
        .get(HEADER)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<AppVersion>().ok());
    if version.is_some_and(|version| version >= min_version) {
        return next.run(req).await;
    }
    AppError::AppUpdateRequired.into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(value: &str) -> AppVersion {
        value.parse().expect("版が不正")
    }

    #[test]
    fn compares_each_part_as_a_number() {
        assert!(v("1.10") > v("1.9"));
        assert!(v("2.0") > v("1.99.99"));
        assert!(v("1.2.1") > v("1.2"));
    }

    #[test]
    fn missing_parts_count_as_zero() {
        assert_eq!(v("1.2"), v("1.2.0"));
    }

    #[test]
    fn rejects_non_numeric_versions() {
        for value in ["", "1.", ".1", "1.a", "+1", "1.2-beta", " 1"] {
            assert!(value.parse::<AppVersion>().is_err(), "{value:?} を通した");
        }
    }
}
