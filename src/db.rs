//! DB 接続とマイグレーションを扱うモジュール。

use std::path::Path;

use sqlx::SqlitePool;
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};

use crate::config;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("データベースへの接続に失敗しました")]
    Connect(#[source] sqlx::Error),
    #[error("マイグレーションに失敗しました")]
    Migrate(#[from] sqlx::migrate::MigrateError),
    #[error("データベース操作に失敗しました")]
    Sqlx(#[from] sqlx::Error),
}

/// `path` の SQLite ファイルに接続する。ファイルが無ければ作成する。
///
/// SQLite はファイル自体は自動作成するが親ディレクトリは作らないため、接続前に作成しておく。
/// 血圧記録という機微な情報を含むため、親ディレクトリと DB ファイル自体を Unix では
/// 所有者のみ読み書き可能にする。
/// WAL (Write-Ahead Logging) にしているのは、複数のブラウザタブなど複数の接続が同時に
/// 読み書きし得るため (デフォルトの DELETE モードだと書き込み中に読み取りがブロックされやすい)。
pub async fn connect(path: &Path) -> Result<SqlitePool, Error> {
    config::create_owner_only_parent_dir(path)
        .map_err(|err| Error::Connect(sqlx::Error::Io(err)))?;

    let options = SqliteConnectOptions::new()
        .filename(path)
        .create_if_missing(true)
        .journal_mode(sqlx::sqlite::SqliteJournalMode::Wal)
        // ADR: sqlx のデフォルトは FULL だが、WAL モードでは NORMAL で十分というのが
        // sqlx/SQLite 公式の推奨。NORMAL は commit ごとの WAL fsync を省くため、
        // アプリのクラッシュには耐えるが、OS クラッシュ/停電時は直近の commit 済み
        // トランザクションがロールバックされ得る (DB 自体は破損しない)。血圧記録の
        // 書き込み頻度は低く、対象は健康記録であって金融取引等ではないため許容する。
        .synchronous(sqlx::sqlite::SqliteSynchronous::Normal)
        .foreign_keys(true)
        // ADR: 消した行の中身を 0 で上書きする。既定のままだと空きページに残り、再利用されるまで DB ファイルと
        // その複製・スナップショットに含まれ続ける。プライバシーポリシーの「消した情報は、複製とスナップショットからも
        // おおむね 8 日のうちに消える」を成り立たせるため (frontend/src/lib/legal/ja/privacy.md)。
        .pragma("secure_delete", "ON");

    let pool = SqlitePoolOptions::new()
        .connect_with(options)
        .await
        .map_err(Error::Connect)?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
            .map_err(|err| Error::Connect(sqlx::Error::Io(err)))?;
    }

    Ok(pool)
}

/// 未適用のマイグレーション (`migrations/`) を実行する。
pub async fn migrate(pool: &SqlitePool) -> Result<(), Error> {
    sqlx::migrate!("./migrations").run(pool).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::validation::{DEFAULT_TIMEZONE, PeriodThresholds};

    // `#[sqlx::test]` は `migrations/` を自動適用した新規 DB を渡してくる。
    #[sqlx::test]
    async fn migration_creates_users_table(pool: SqlitePool) {
        let count: i64 = sqlx::query_scalar!(
            "SELECT count(*) AS \"count!: i64\" FROM sqlite_master WHERE type = 'table' AND name = 'users'"
        )
        .fetch_one(&pool)
        .await
        .expect("failed to query sqlite_master");
        assert_eq!(count, 1);
    }

    #[sqlx::test]
    async fn migration_creates_bp_records_table(pool: SqlitePool) {
        let count: i64 = sqlx::query_scalar!(
            "SELECT count(*) AS \"count!: i64\" FROM sqlite_master WHERE type = 'table' AND name = 'bp_records'"
        )
        .fetch_one(&pool)
        .await
        .expect("failed to query sqlite_master");
        assert_eq!(count, 1);
    }

    /// 朝/夜の時間帯設定を追加したマイグレーションのバージョン。
    const PERIOD_SETTINGS_VERSION: i64 = 20260909100000;

    /// マイグレーションの列 DEFAULT が Rust 側の既定値と一致することを固定する。ずれても
    /// 実行時エラーにはならず集計の対象時間帯だけが変わるため、テストで押さえる。
    fn default_period_settings() -> (&'static str, i64, i64, i64, i64) {
        let t = PeriodThresholds::DEFAULT;
        (
            DEFAULT_TIMEZONE,
            t.morning_start_min,
            t.morning_end_min,
            t.evening_start_min,
            t.evening_end_min,
        )
    }

    /// `migrations/` を `version` の手前まで適用する。列を追加する前の状態を作って、
    /// 適用の前後で既存行がどうなるかを確かめるために使う。
    async fn apply_migrations_before(pool: &SqlitePool, version: i64) {
        use sqlx::migrate::Migrate;

        let mut conn = pool
            .acquire()
            .await
            .expect("failed to acquire a connection");
        conn.ensure_migrations_table()
            .await
            .expect("failed to create the migrations table");
        for migration in sqlx::migrate!("./migrations")
            .iter()
            .filter(|m| m.version < version)
        {
            conn.apply(migration)
                .await
                .expect("failed to apply a migration");
        }
    }

    /// 朝/夜の設定列がまだ無い時点でも通るよう、追加列に触れない INSERT にしてある。
    async fn insert_bare_user(pool: &SqlitePool) {
        sqlx::query!("INSERT INTO users (username, password_hash) VALUES ('u', 'x')")
            .execute(pool)
            .await
            .expect("failed to insert a test user");
    }

    async fn read_period_settings(pool: &SqlitePool) -> (String, i64, i64, i64, i64) {
        let row = sqlx::query!(
            r#"
            SELECT
                timezone AS "timezone!: String",
                morning_start_min AS "morning_start_min!: i64",
                morning_end_min AS "morning_end_min!: i64",
                evening_start_min AS "evening_start_min!: i64",
                evening_end_min AS "evening_end_min!: i64"
            FROM users
            "#
        )
        .fetch_one(pool)
        .await
        .expect("failed to read the user back");
        (
            row.timezone,
            row.morning_start_min,
            row.morning_end_min,
            row.evening_start_min,
            row.evening_end_min,
        )
    }

    fn assert_default_period_settings(settings: (String, i64, i64, i64, i64)) {
        let (timezone, morning_start, morning_end, evening_start, evening_end) = settings;
        assert_eq!(
            (
                timezone.as_str(),
                morning_start,
                morning_end,
                evening_start,
                evening_end
            ),
            default_period_settings()
        );
    }

    #[sqlx::test]
    async fn migration_gives_new_users_the_default_period_settings(pool: SqlitePool) {
        insert_bare_user(&pool).await;

        assert_default_period_settings(read_period_settings(&pool).await);
    }

    // 列を後から足す変更では、新規行が既定値を持っていても既存行が同じとは限らない。
    // 追加前に作られたユーザーが朝/夜の集計から漏れないことを、適用の前後で確認する。
    #[sqlx::test(migrations = false)]
    async fn migration_backfills_existing_users_with_the_default_period_settings(pool: SqlitePool) {
        apply_migrations_before(&pool, PERIOD_SETTINGS_VERSION).await;
        insert_bare_user(&pool).await;

        migrate(&pool).await.expect("migrate should succeed");

        assert_default_period_settings(read_period_settings(&pool).await);
    }

    /// 削除予約の起点を追加し、予定日時をミリ秒の形に揃えたマイグレーションのバージョン。
    const DELETION_ORIGIN_VERSION: i64 = 20260913200000;

    // 既存の予約は、列ができる前の挙動 (本人のログインで取り消せる) に合わせて本人が起点になり、
    // 予定日時は期限の判定 (文字列の大小) が崩れないようミリ秒の形に揃う。
    #[sqlx::test(migrations = false)]
    async fn migration_backfills_existing_deletion_schedules(pool: SqlitePool) {
        apply_migrations_before(&pool, DELETION_ORIGIN_VERSION).await;
        sqlx::query!(
            "INSERT INTO users (username, password_hash, deletion_scheduled_at)
             VALUES ('scheduled', 'x', '2030-01-01T00:00:00Z'), ('unscheduled', 'x', NULL)"
        )
        .execute(&pool)
        .await
        .expect("failed to insert test users");

        migrate(&pool).await.expect("migrate should succeed");

        let rows = sqlx::query!(
            r#"SELECT username, deletion_scheduled_at AS "deletion_scheduled_at: String",
                      deletion_origin AS "deletion_origin: String"
               FROM users ORDER BY username"#
        )
        .fetch_all(&pool)
        .await
        .expect("failed to read users back");
        let rows: Vec<_> = rows
            .into_iter()
            .map(|row| (row.username, row.deletion_scheduled_at, row.deletion_origin))
            .collect();
        assert_eq!(
            rows,
            vec![
                (
                    "scheduled".to_string(),
                    Some("2030-01-01T00:00:00.000Z".to_string()),
                    Some("user".to_string())
                ),
                ("unscheduled".to_string(), None, None),
            ]
        );
    }

    #[sqlx::test]
    async fn migrate_is_idempotent(pool: SqlitePool) {
        migrate(&pool)
            .await
            .expect("re-running migrate should be a no-op, not an error");
    }

    /// 消した行を上書きする設定が、どの接続にも効いている (プライバシーポリシーの削除の期間の前提)。
    #[tokio::test]
    async fn connect_enables_secure_delete() {
        let tmp = crate::test_support::TempDir::new("db-secure-delete");
        let pool = connect(&tmp.path().join("test.db"))
            .await
            .expect("connect should succeed");

        let enabled = sqlx::query_scalar!("PRAGMA secure_delete")
            .fetch_one(&pool)
            .await
            .expect("PRAGMA secure_delete should succeed");
        pool.close().await;

        assert_eq!(enabled, Some(1));
    }

    #[tokio::test]
    async fn connect_creates_parent_dir_and_file() {
        let tmp = crate::test_support::TempDir::new("db-connect");
        let path = tmp.path().join("nested").join("test.db");

        let pool = connect(&path).await.expect("connect should succeed");
        migrate(&pool).await.expect("migrate should succeed");
        pool.close().await;

        assert!(path.exists());
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn connect_makes_parent_dir_and_file_owner_only() {
        use std::os::unix::fs::PermissionsExt;

        let tmp = crate::test_support::TempDir::new("db-owner-only");
        let parent = tmp.path().join("nested");
        let path = parent.join("test.db");

        let pool = connect(&path).await.expect("connect should succeed");
        pool.close().await;

        let dir_mode = std::fs::metadata(&parent)
            .expect("failed to read the parent dir metadata")
            .permissions()
            .mode();
        assert_eq!(
            dir_mode & 0o777,
            0o700,
            "親ディレクトリは所有者のみアクセス可能であるべき"
        );

        let file_mode = std::fs::metadata(&path)
            .expect("failed to read the db file metadata")
            .permissions()
            .mode();
        assert_eq!(
            file_mode & 0o777,
            0o600,
            "DB ファイルは所有者のみ読み書き可能であるべき"
        );
    }
}
