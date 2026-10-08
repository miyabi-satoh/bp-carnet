//! シングルインスタンス化。
//!
//! `BP_CARNET_HOME` (または OS 標準のデータディレクトリ) が同じまま、`config.toml` の
//! `port` を変えて誤って複数回起動する、`just dev-backend` を二重に実行してしまう、といった
//! ケースで同じ DB・セッション鍵に複数プロセスが同時アクセスするのを防ぐ。データディレクトリ
//! 配下のロックファイルに対する OS のファイルロック (Unix: flock, Windows: LockFileEx) で
//! 二重起動を検知する。ロックはプロセスが (正常終了・クラッシュを問わず) 終了して該当ファイル
//! ハンドルが閉じられれば OS 側で自動的に解放されるため、ロックファイル自体の掃除は不要。

use std::fs::{File, OpenOptions, TryLockError};
use std::path::Path;

use crate::config;

/// シングルインスタンスロックの取得を試みる。
///
/// `File::try_lock` (Rust 1.89 で安定化) を使うため、専用の依存クレートは不要。
/// 返り値の `File` はロックの生存期間を握っている。呼び出し元はプロセス終了まで
/// これを保持し続けること (drop するとロックが解放される)。既に別プロセスが
/// ロックを保持している場合は `Ok(None)` を返す (これはエラーではない)。
pub fn acquire(path: &Path) -> std::io::Result<Option<File>> {
    // ロックファイルの親はデータディレクトリそのもの (DB・セッション鍵と同居する)。
    config::create_owner_only_parent_dir(path)?;
    // ロック取得だけが目的でファイルの中身は使わないため、既存の内容は保持する
    // (truncate すると、ロック取得中の別プロセスがいた場合に無意味な書き込みになる)。
    let file = OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(false)
        .open(path)?;
    match file.try_lock() {
        Ok(()) => Ok(Some(file)),
        Err(TryLockError::WouldBlock) => Ok(None),
        Err(TryLockError::Error(err)) => Err(err),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::TempDir;

    #[test]
    fn first_caller_acquires_and_second_is_rejected() {
        let tmp = TempDir::new("single-instance-second-caller");
        std::fs::create_dir_all(tmp.path()).expect("failed to create the temp dir");
        let path = tmp.path().join("test.lock");

        let first = acquire(&path).expect("first acquire should not error");
        assert!(first.is_some(), "最初の取得はロックを保持できるはず");

        let second = acquire(&path).expect("second acquire should not error");
        assert!(
            second.is_none(),
            "ロック保持中の別ハンドルからの取得は None になるはず"
        );

        drop(first);
        let third = acquire(&path).expect("third acquire should not error");
        assert!(third.is_some(), "解放後は再度取得できるはず");
    }

    #[cfg(unix)]
    #[test]
    fn acquire_creates_owner_only_parent_dir() {
        use std::os::unix::fs::PermissionsExt;

        // 親ディレクトリが未作成の状態から呼ぶ (config.rs の同種のテストと同じ狙い):
        // `create_dir_all` に差し戻すリグレッションを検知する。
        let tmp = TempDir::new("single-instance-owner-only");
        let path = tmp.path().join("test.lock");

        let _lock = acquire(&path).expect("acquire should not error");

        let mode = std::fs::metadata(tmp.path())
            .expect("failed to read the lock dir metadata")
            .permissions()
            .mode();
        assert_eq!(
            mode & 0o777,
            0o700,
            "ロックファイルの親ディレクトリは所有者のみアクセス可能であるべき"
        );
    }
}
