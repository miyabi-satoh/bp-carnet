//! アプリの LINE・Apple ログインで使う `nonce` (docs/mobile-app.md)。
//!
//! ログインを始めるたびにサーバーが発行し、アプリはそれを各 SDK に渡す。ID トークンに載って
//! 戻ってきた値を [`NonceStore::consume`] で照らし合わせ、一度使ったら消す。
//! 同じ ID トークンを別の場面で使い回されるのを防ぐため。
//!
//! 保持はプロセスのメモリだけ (`crate::auth::AttemptRateLimiter` と同じ割り切り)。再起動すると
//! 発行済みの値は使えなくなるが、ログインをやり直せば済む。

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// ADR: 発行から使えるまでの時間。SDK の画面で ID・パスワードを打ち込んだり、LINE アプリとの
/// 行き来を挟んだりしても収まる長さにし、それ以上は持たない。
const TTL: Duration = Duration::from_secs(10 * 60);

/// ADR: 同時に持つ数の上限。発行は未認証で叩けるので、メモリを際限なく使わせない。
/// 接続元ごとの発行の制限 (`AppState::app_nonce_rate_limiter`) と合わせて、
/// ふつうの利用ではまず届かない値にする。
const MAX_PENDING: usize = 10_000;

pub struct NonceStore {
    pending: Mutex<HashMap<String, Instant>>,
    ttl: Duration,
    max_pending: usize,
}

impl Default for NonceStore {
    fn default() -> Self {
        Self::new(TTL, MAX_PENDING)
    }
}

impl NonceStore {
    fn new(ttl: Duration, max_pending: usize) -> Self {
        Self {
            pending: Mutex::new(HashMap::new()),
            ttl,
            max_pending,
        }
    }

    /// 新しい `nonce` を発行する。持てる数の上限に達していれば `None`。
    pub fn issue(&self) -> Option<String> {
        let now = Instant::now();
        // 別のリクエストがロック中に panic しても使い続ける (`AttemptRateLimiter` と同じ)。
        let mut pending = self.pending.lock().unwrap_or_else(|e| e.into_inner());
        pending.retain(|_, expires_at| *expires_at > now);
        if pending.len() >= self.max_pending {
            return None;
        }
        let nonce = crate::external_login::generate_nonce();
        pending.insert(nonce.clone(), now + self.ttl);
        Some(nonce)
    }

    /// 発行済みで期限内の `nonce` なら消して `true`。同じ値で2回目は `false`。
    pub fn consume(&self, nonce: &str) -> bool {
        let mut pending = self.pending.lock().unwrap_or_else(|e| e.into_inner());
        pending
            .remove(nonce)
            .is_some_and(|expires_at| expires_at > Instant::now())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn issued_nonce_can_be_consumed_only_once() {
        let store = NonceStore::default();
        let nonce = store.issue().expect("上限に達していないので発行できるはず");
        assert!(nonce.chars().all(|c| c.is_ascii_alphanumeric()), "{nonce}");
        assert!(store.consume(&nonce));
        assert!(!store.consume(&nonce));
    }

    #[test]
    fn unknown_nonce_is_rejected() {
        let store = NonceStore::default();
        assert!(!store.consume("not-issued"));
    }

    #[test]
    fn expired_nonce_is_rejected() {
        let store = NonceStore::new(Duration::ZERO, 10);
        let nonce = store.issue().expect("発行できるはず");
        assert!(!store.consume(&nonce));
    }

    #[test]
    fn issue_stops_at_the_limit_and_resumes_after_expiry() {
        let store = NonceStore::new(Duration::from_secs(60), 2);
        assert!(store.issue().is_some());
        assert!(store.issue().is_some());
        assert!(store.issue().is_none());

        let store = NonceStore::new(Duration::ZERO, 1);
        assert!(store.issue().is_some());
        // 期限切れの分は発行のときに掃除されるので、上限に数えない。
        assert!(store.issue().is_some());
    }
}
