//! 複数モジュールの `#[cfg(test)]` から使うヘルパー。
//! 統合テスト (`tests/`) は別クレートで `cfg(test)` の項目を参照できないため、そちらは各自で持つ。

use std::path::{Path, PathBuf};

use sqlx::SqlitePool;
use sqlx::pool::PoolConnection;

use crate::auth::{self, NewPassword, Role};
use crate::payments::ocr_quota;

/// テストのためだけに作った鍵 (Apple には登録していない)。
pub(crate) const TEST_PRIVATE_KEY: &str = "-----BEGIN PRIVATE KEY-----
MIGHAgEAMBMGByqGSM49AgEGCCqGSM49AwEHBG0wawIBAQQgxxiPfi/vjhWHeioS
mETQ4ZhL0FsiWicWx83WNb1HJZmhRANCAATC/ECNudnDQqIEF3MsxKUHdgOyKcUk
meRzhDnSIYMONHYscf1pwLzCmbxcVSREfbcX7AN1Mb+SNerfKhE6qh/a
-----END PRIVATE KEY-----
";

/// テスト用の一時ディレクトリ (外部クレートを増やさないための自前実装)。パスを決めるだけで作らず、
/// drop で消す (途中で panic しても残さない)。作られることを確かめるテストのため、作るのは呼ぶ側に任せる。
pub(crate) struct TempDir(PathBuf);

impl TempDir {
    /// 並行して走る他のテストと衝突しないよう、`name` にスレッドを添えて名前を決める。
    pub(crate) fn new(name: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "bp-carnet-test-{}-{name}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&path);
        Self(path)
    }

    pub(crate) fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// 接続を1本取り出す (`&mut *conn` を executor に渡す関数のテスト向け)。
pub(crate) async fn conn(pool: &SqlitePool) -> PoolConnection<sqlx::Sqlite> {
    pool.acquire().await.expect("接続を取れるはず")
}

/// 一般ユーザーを作って id を返す。パスワードを照合しないテスト向けに、ハッシュ化を省いた固定値を入れる。
pub(crate) async fn insert_user(pool: &SqlitePool, username: &str) -> i64 {
    create(pool, username, NewPassword::Usable("dummy-hash")).await
}

/// `password` でログインできる一般ユーザーを作って id を返す。
pub(crate) async fn insert_user_with_password(
    pool: &SqlitePool,
    username: &str,
    password: &str,
) -> i64 {
    let hash = auth::hash_password(password).expect("パスワードをハッシュ化できるはず");
    create(pool, username, NewPassword::Usable(&hash)).await
}

async fn create(pool: &SqlitePool, username: &str, password: NewPassword<'_>) -> i64 {
    auth::create_user(pool, username, password, Role::User)
        .await
        .expect("テストユーザーを作成できるはず")
}

/// テストで使う、買い足し1回で付ける量 (1/1000円)。売値で割り切れる値にして、売値に直した額を読みやすくする。
pub(crate) const TEST_TOPUP_GRANT_MILLI_YEN: i64 = 60_000;

/// 買い足し枠を1件入れる。`remaining_milli_yen` はその枠の残り (付与額は [`TEST_TOPUP_GRANT_MILLI_YEN`])。
pub(crate) async fn insert_ocr_quota_grant(
    pool: &SqlitePool,
    user_id: i64,
    checkout_session_id: &str,
    remaining_milli_yen: i64,
) {
    sqlx::query!(
        "INSERT INTO ocr_quota_grants (user_id, granted_milli_yen, remaining_milli_yen, purchased_at, price_yen, stripe_checkout_session_id) VALUES (?, ?, ?, ?, 300, ?)",
        user_id,
        TEST_TOPUP_GRANT_MILLI_YEN,
        remaining_milli_yen,
        "2026-01-01T00:00:00.000Z",
        checkout_session_id
    )
    .execute(pool)
    .await
    .expect("grant should be inserted");
}

/// 購入の枠の残りの合計 (1/1000円)。
pub(crate) async fn paid_remaining(pool: &SqlitePool, user_id: i64) -> i64 {
    ocr_quota::available(pool, user_id)
        .await
        .expect("grants should be readable")
        .iter()
        .map(|grant| grant.remaining_milli_yen)
        .sum()
}
