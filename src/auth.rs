//! パスワードハッシュ化・検証、ログイン試行のレートリミット、認証済みユーザーの extractor。

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{LazyLock, Mutex};
use std::time::{Duration, Instant};

use argon2::password_hash::PasswordHasher;
use argon2::{Argon2, PasswordVerifier};
use axum::extract::FromRequestParts;
use axum::http::request::Parts;
use serde::{Deserialize, Serialize};
use sqlx::{SqliteConnection, SqliteExecutor, SqlitePool};
use tower_sessions::Session;
use utoipa::ToSchema;

use crate::config::{CharClass, PasswordConfig};
use crate::error::AppError;
use crate::state::AppState;

/// セッションに認証済みユーザーの DB 上の id (`users.id`) を保持する際のキー。
pub const SESSION_USER_ID_KEY: &str = "user_id";

/// セッションに持たせる `users.session_generation` のコピー。値が DB 側とずれたセッションは
/// 無効として扱う (docs/authentication.md)。
pub const SESSION_GENERATION_KEY: &str = "session_generation";

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("パスワードのハッシュ化に失敗しました")]
    Hash(#[source] argon2::password_hash::Error),
    #[error("パスワードハッシュの検証に失敗しました")]
    Verify(#[source] argon2::password_hash::Error),
    /// `spawn_blocking` したタスクが panic 等で join できなかった場合。
    #[error("パスワード検証タスクの完了待ちに失敗しました")]
    Join(#[source] tokio::task::JoinError),
    /// セッションストア (DB) 側のエラー。クライアント起因ではないので 5xx として扱う
    /// (`AppError::Internal` 経由)。「未ログイン」の 401 とは明確に区別する。
    #[error("セッション操作に失敗しました")]
    Session(#[from] tower_sessions::session::Error),
    /// `SessionManagerLayer` が掛かっていないルートで `Session` を抽出しようとした場合。
    /// 実際には到達しないはず (常にレイヤーを掛けているため) だが、設定ミスとして型で表現しておく。
    #[error("セッションレイヤーが設定されていません: {0}")]
    MissingSessionLayer(&'static str),
}

/// ユーザーの権限 (docs/authentication.md)。`users.role` の 'user' | 'admin' に対応する。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, sqlx::Type, ToSchema)]
#[serde(rename_all = "lowercase")]
#[sqlx(rename_all = "lowercase")]
pub enum Role {
    User,
    Admin,
}

/// 認証済みユーザーを表す。
#[derive(Debug)]
pub struct User {
    pub id: i64,
    pub username: String,
    /// 凍結中か (docs/authentication.md)。パスワードは合っていても、呼び出し側がログインを拒む。
    pub frozen: bool,
}

struct UserRow {
    id: i64,
    username: String,
    password_hash: String,
    frozen: i64,
}

/// `username` を照合・保存に使う正規形に正規化する。
///
/// username にはメールアドレス (OAuth・独自ID/PW) と任意のユーザーID の両方が入りうるが、
/// 書式で分岐させず常に小文字化する (docs/data-model.md)。作成時だけで
/// なく、ログイン照合やレートリミッタのキーなど username を検索条件に使う全ての箇所で
/// この関数を通すこと。
///
/// ASCII 限定 (`to_ascii_lowercase`) なのは、既存行を小文字化するマイグレーションが使う
/// SQLite の `lower()` が ASCII のみを対象とするため。Rust 側で Unicode 小文字化まで
/// 行うと、マイグレーション済みの既存行と新規行で正規化結果がずれる。
pub fn normalize_username(username: &str) -> String {
    username.to_ascii_lowercase()
}

/// 作成するユーザーのパスワード。`password_hash` と `password_usable` を必ず対で決めるため、
/// ハッシュ単体ではなくこの形で受け取る。
#[derive(Debug, Clone, Copy)]
pub enum NewPassword<'a> {
    /// ID/PW ログインに使えるハッシュ。
    Usable(&'a str),
    /// OAuth 専用アカウント。`password_hash` は NOT NULL のため、絶対に一致しないハッシュ
    /// (ランダムな平文のハッシュ) を渡す。
    Unusable(&'a str),
}

impl<'a> NewPassword<'a> {
    fn hash(self) -> &'a str {
        match self {
            Self::Usable(hash) | Self::Unusable(hash) => hash,
        }
    }

    fn usable(self) -> bool {
        matches!(self, Self::Usable(_))
    }
}

/// ユーザーを1件作成する。`username` は [`normalize_username`] を通してから保存する。
///
/// 呼び出し元 (`--create-user`・管理画面からの追加・Google ログイン) はどれもメールアドレスを確認済みと
/// して扱う経路なので、`email_verified = 1` で保存する。`executor` はプール本体・トランザクションの
/// どちらでも渡せる。
///
/// UNIQUE 違反は `sqlx::Error::Database` としてそのまま返すため、呼び出し側で
/// `is_unique_violation()` により「既に存在する」を判別できる。
pub async fn create_user<'e, E: SqliteExecutor<'e>>(
    executor: E,
    username: &str,
    password: NewPassword<'_>,
    role: Role,
) -> Result<i64, sqlx::Error> {
    let username = normalize_username(username);
    let password_hash = password.hash();
    let password_usable = password.usable();
    let result = sqlx::query!(
        "INSERT INTO users (username, password_hash, password_usable, email_verified, role)
         VALUES (?, ?, ?, 1, ?)",
        username,
        password_hash,
        password_usable,
        role
    )
    .execute(executor)
    .await?;
    Ok(result.last_insert_rowid())
}

/// `username` の未確認のアカウント (サインアップの確認待ち、`crate::signup`) があれば削除する。
/// 管理者がユーザーを作る直前に呼ぶ。未確認のアカウントはパスワードも記録も持たず管理画面にも
/// 出ないため、残したままだと理由の分からない「既に使われている」になるうえ、他人が申し込み直す
/// たびに期限が延びて消えないことがある。
pub async fn discard_unverified_user<'e, E: SqliteExecutor<'e>>(
    executor: E,
    username: &str,
) -> Result<(), sqlx::Error> {
    let username = normalize_username(username);
    sqlx::query!(
        "DELETE FROM users WHERE username = ? AND email_verified = 0",
        username
    )
    .execute(executor)
    .await?;
    Ok(())
}

/// 指定した id の、メール確認済みのユーザーが存在するか。管理者の操作は、未確認のアカウントを
/// 存在しないものとして扱う (docs/authentication.md)。
pub async fn verified_user_exists<'e, E: SqliteExecutor<'e>>(
    executor: E,
    user_id: i64,
) -> Result<bool, sqlx::Error> {
    sqlx::query_scalar!(
        r#"SELECT EXISTS (SELECT 1 FROM users WHERE id = ? AND email_verified = 1) AS "exists!: bool""#,
        user_id
    )
    .fetch_one(executor)
    .await
}

/// `username` (正規化済み) の、メール確認済みのユーザーの id。未確認のアカウントは、無いものとして扱う。
pub async fn find_verified_user_id<'e, E: SqliteExecutor<'e>>(
    executor: E,
    username: &str,
) -> Result<Option<i64>, sqlx::Error> {
    sqlx::query_scalar!(
        r#"SELECT id AS "id!: i64" FROM users WHERE username = ? AND email_verified = 1"#,
        username
    )
    .fetch_optional(executor)
    .await
}

/// `users` に保存されている、画面の表示に使うプロフィール。
pub struct Profile {
    /// Google ログインで取得した名前 (独自アカウントは `None`)。
    pub display_name: Option<String>,
    /// Google ログインで取得したプロフィール画像の URL (独自アカウントは `None`)。
    pub avatar_url: Option<String>,
    /// ID/PW ログインに使えるパスワードを持つか (`users.password_usable`)。
    pub password_usable: bool,
}

/// `user_id` のユーザー ID。ユーザーが居なければ `None`。
pub async fn load_username(pool: &SqlitePool, user_id: i64) -> Result<Option<String>, sqlx::Error> {
    sqlx::query_scalar!("SELECT username FROM users WHERE id = ?", user_id)
        .fetch_optional(pool)
        .await
}

pub async fn load_profile(pool: &SqlitePool, user_id: i64) -> Result<Profile, sqlx::Error> {
    let row = sqlx::query!(
        "SELECT display_name, avatar_url, password_usable FROM users WHERE id = ?",
        user_id
    )
    .fetch_one(pool)
    .await?;
    Ok(Profile {
        display_name: row.display_name,
        avatar_url: row.avatar_url,
        password_usable: row.password_usable != 0,
    })
}

/// 保存されているパスワードハッシュ。OAuth 専用アカウントでは、絶対に一致しない値が入っている。
pub async fn load_password_hash(pool: &SqlitePool, user_id: i64) -> Result<String, sqlx::Error> {
    sqlx::query_scalar!("SELECT password_hash FROM users WHERE id = ?", user_id)
        .fetch_one(pool)
        .await
}

/// 本人によるパスワード変更 (docs/authentication.md)。新しいセッションの世代を返す。
/// `password_usable` は更新しない: 現在のパスワードの照合を通るのは、既にパスワードを持つ
/// ユーザーだけのため。
///
/// ADR: 世代の更新をハッシュの更新と同じ1文に入れる。別々の文にすると、ハッシュだけが
/// 変わって世代が据え置かれた状態 (乗っ取った相手のセッションが生き残る) がありうる。
///
/// アプリから変えたとき (`app_token_id`) は、そのトークンも同じトランザクションで新しい世代に
/// 載せ替える (docs/mobile-app.md)。別々にすると、間に同じ端末の別のリクエストが古い世代の
/// トークンを見つけて消し、変えた端末まで断たれる。
pub async fn update_password_hash(
    pool: &SqlitePool,
    user_id: i64,
    password_hash: &str,
    app_token_id: Option<i64>,
) -> Result<i64, sqlx::Error> {
    let mut tx = pool.begin().await?;
    let generation = sqlx::query_scalar!(
        r#"UPDATE users SET password_hash = ?, session_generation = session_generation + 1
           WHERE id = ?
           RETURNING session_generation AS "session_generation!: i64""#,
        password_hash,
        user_id
    )
    .fetch_one(&mut *tx)
    .await?;
    if let Some(app_token_id) = app_token_id {
        sqlx::query!(
            "UPDATE app_tokens SET session_generation = ? WHERE id = ? AND user_id = ?",
            generation,
            app_token_id,
            user_id
        )
        .execute(&mut *tx)
        .await?;
    }
    tx.commit().await?;
    Ok(generation)
}

/// 管理者によるパスワード再設定の結果 (docs/authentication.md)。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PasswordReset {
    Done,
    /// 対象がパスワードを持たない (Google 専用アカウント)。新しく持たせるのは本人の
    /// メール確認が前提なので、管理者からは設定しない。
    NoPassword,
    /// 対象が存在しない (未確認のアカウントを含む)。
    NotFound,
}

/// 管理者によるパスワードの再設定 (docs/authentication.md)。対象のセッションはすべて断たれる
/// (`session_generation` を上げる)。本人による変更と違い、操作しているのは別人なので、
/// 生き残らせるセッションが無い。
///
/// 更新と、更新できなかった理由の読み取りを1つのトランザクションに収めたいので、プールでは
/// なく接続を受け取る。監査ログの記録も同じトランザクションに載せる。
///
/// ADR: 対象の状態を先に読まず、書き込みを最初の文にする。deferred なトランザクションで
/// 読みを先に置くと、後から書き込みへ昇格する際に `SQLITE_BUSY_SNAPSHOT` になりうる
/// (`account::schedule_deletion` と同じ理由)。
pub async fn reset_password_hash(
    conn: &mut SqliteConnection,
    user_id: i64,
    password_hash: &str,
) -> Result<PasswordReset, sqlx::Error> {
    let updated = sqlx::query!(
        "UPDATE users SET password_hash = ?, session_generation = session_generation + 1
         WHERE id = ? AND password_usable = 1 AND email_verified = 1",
        password_hash,
        user_id
    )
    .execute(&mut *conn)
    .await?;
    if updated.rows_affected() > 0 {
        return Ok(PasswordReset::Done);
    }

    // 更新できなかった理由を、存在しないのかパスワードを持たないのかに分ける。
    Ok(if verified_user_exists(&mut *conn, user_id).await? {
        PasswordReset::NoPassword
    } else {
        PasswordReset::NotFound
    })
}

/// ログイン成功時にセッションを確立する。ID/PW ログインと Google ログインの両方から呼ぶ。
///
/// セッション固定攻撃対策として、書き込む前にセッション ID を更新する (docs/authentication.md)。
/// 世代を一緒に持たせるのは、パスワード変更で他の端末のセッションを無効にするため。
pub async fn establish_session(
    session: &Session,
    pool: &SqlitePool,
    user_id: i64,
) -> Result<(), AppError> {
    let generation = sqlx::query_scalar!(
        r#"SELECT session_generation AS "session_generation!: i64" FROM users WHERE id = ?"#,
        user_id
    )
    .fetch_one(pool)
    .await?;

    // ログインも、使われていることの記録に数える (docs/authentication.md)。
    crate::account::touch_last_seen(pool, user_id).await?;

    session.cycle_id().await.map_err(Error::Session)?;
    session
        .insert(SESSION_USER_ID_KEY, user_id)
        .await
        .map_err(Error::Session)?;
    session
        .insert(SESSION_GENERATION_KEY, generation)
        .await
        .map_err(Error::Session)?;
    Ok(())
}

/// 設定のポリシー (`[password]`、docs/authentication.md) を満たさないパスワード。
#[derive(Debug, PartialEq, Eq, thiserror::Error)]
pub enum PasswordPolicyError {
    #[error("パスワードは{min_length}文字以上にしてください")]
    TooShort { min_length: usize },
    #[error("パスワードには{}を含めてください", .missing.iter().map(|class| class.label()).collect::<Vec<_>>().join("・"))]
    MissingClasses { missing: Vec<CharClass> },
}

/// パスワードが設定のポリシーを満たすか検証する。パスワードを設定する経路すべてが
/// ここを通る。
///
/// 文字数は書記素クラスタではなくコードポイントで数える。絵文字のように複数の
/// コードポイントで1文字になる入力は実質的に有利側 (長く数える) に倒れるが、
/// 最小長を下回らせないという目的は満たせる。
pub fn validate_password(
    config: &PasswordConfig,
    password: &str,
) -> Result<(), PasswordPolicyError> {
    if password.chars().count() < config.min_length {
        return Err(PasswordPolicyError::TooShort {
            min_length: config.min_length,
        });
    }
    let missing: Vec<CharClass> = config
        .required_classes
        .iter()
        .copied()
        .filter(|class| !class.contained_in(password))
        .collect();
    if !missing.is_empty() {
        return Err(PasswordPolicyError::MissingClasses { missing });
    }
    Ok(())
}

/// パスワードを Argon2id でハッシュ化する (salt は内部で生成される)。
pub fn hash_password(password: &str) -> Result<String, Error> {
    Argon2::default()
        .hash_password(password.as_bytes())
        .map(|hash| hash.to_string())
        .map_err(Error::Hash)
}

/// [`hash_password`] をワーカースレッドの外で行う。Argon2id は CPU バウンドなため、非同期の
/// ハンドラから直接呼ぶと他のリクエストを塞ぐ。
pub async fn hash_password_async(password: String) -> Result<String, Error> {
    tokio::task::spawn_blocking(move || hash_password(&password))
        .await
        .map_err(Error::Join)?
}

/// ID/PW ログインで絶対にマッチしない「使用不能」なパスワードハッシュを生成する
/// (Django の `set_unusable_password` 相当)。パスワードを持たないアカウント (Google 専用・
/// サインアップの確認待ち) の `users.password_hash` (NOT NULL のため空にできない) に入れる。
pub async fn unusable_password_hash() -> Result<String, Error> {
    hash_password_async(crate::token::random_url_safe(32)).await
}

/// パスワードを検証する。`Ok(false)` は「単に不一致」、`Err` は DB 内のハッシュが壊れている
/// 等の想定外の失敗で、呼び出し側で区別できるようにしている
/// (両方を `false` にまとめると、壊れたハッシュが「パスワード不一致」として握り潰されてしまう)。
pub fn verify_password(password: &str, hash: &str) -> Result<bool, Error> {
    match Argon2::default().verify_password(password.as_bytes(), hash) {
        Ok(()) => Ok(true),
        Err(argon2::password_hash::Error::PasswordInvalid) => Ok(false),
        Err(err) => Err(Error::Verify(err)),
    }
}

/// パスワードを検証する ([`verify_password`] を `spawn_blocking` で回す)。
pub async fn verify_password_async(password: String, hash: String) -> Result<bool, Error> {
    tokio::task::spawn_blocking(move || verify_password(&password, &hash))
        .await
        .map_err(Error::Join)?
}

/// 現在のパスワードを照合し、一致すれば新しいパスワードのハッシュを返す (不一致なら `None`)。
/// Argon2 の検証とハッシュ化はどちらも CPU バウンドなため、まとめて `spawn_blocking` に寄せる。
pub async fn verify_and_hash_password(
    current_password: String,
    current_hash: String,
    new_password: String,
) -> Result<Option<String>, Error> {
    tokio::task::spawn_blocking(move || {
        if !verify_password(&current_password, &current_hash)? {
            return Ok(None);
        }
        hash_password(&new_password).map(Some)
    })
    .await
    .map_err(Error::Join)?
}

/// ユーザーが存在しない場合の検証にも使うダミーハッシュ。
/// 実在しないユーザーへのログイン試行でも、実在するユーザーのパスワード不一致と同程度の
/// 時間がかかるようにし、ユーザー名の存在有無がレスポンス時間から漏れないようにする。
/// 実ハッシュと同じパラメータで生成するため、`Argon2::default()` の既定値が変わっても
/// コストが揃う。
static DUMMY_HASH: LazyLock<String> = LazyLock::new(|| {
    hash_password("dummy-password-for-timing-safety").expect("failed to hash dummy password")
});

/// ユーザー名・パスワードを検証する。ユーザーが存在しない場合はダミーハッシュと照合してから
/// `None` を返す (タイミング攻撃対策)。
///
/// Argon2 の検証は CPU バウンドな処理のため `spawn_blocking` で非同期ランタイムをブロック
/// しないようにする。
pub async fn authenticate(
    pool: &SqlitePool,
    username: &str,
    password: &str,
) -> Result<Option<User>, AppError> {
    let username = normalize_username(username);
    let row = sqlx::query_as!(
        UserRow,
        // 未確認のアカウント (オープンサインアップの確認待ち) は存在しないものとして扱う。
        r#"SELECT id AS "id!: i64", username, password_hash, frozen FROM users
           WHERE username = ? AND email_verified = 1"#,
        username
    )
    .fetch_optional(pool)
    .await?;

    let password = password.to_string();
    let user = tokio::task::spawn_blocking(move || -> Result<Option<User>, Error> {
        match row {
            Some(row) => {
                let ok = verify_password(&password, &row.password_hash)?;
                Ok(ok.then_some(User {
                    id: row.id,
                    username: row.username,
                    frozen: row.frozen != 0,
                }))
            }
            None => {
                // 必ず false になる (対応する平文を知らない) が、時間だけは揃える。
                verify_password(&password, &DUMMY_HASH)?;
                Ok(None)
            }
        }
    })
    .await
    .map_err(Error::Join)??;

    Ok(user)
}

/// レートリミッタが Map のキーとして追跡するユーザー名長の上限 (バイト数)。
/// 制限が無いと、巨大な username を大量に送るだけでメモリを圧迫できてしまう。
/// この長さで打ち切って集計するため、冒頭がこの長さと一致する別々の username は同じ試行回数
/// として扱われるが、レートリミット用途では許容できるトレードオフとする。
const MAX_TRACKED_USERNAME_LEN: usize = 256;

/// UTF-8 の文字境界を壊さずに `MAX_TRACKED_USERNAME_LEN` 以内に切り詰める。
fn truncate_username(username: &str) -> &str {
    if username.len() <= MAX_TRACKED_USERNAME_LEN {
        return username;
    }
    let mut end = MAX_TRACKED_USERNAME_LEN;
    while !username.is_char_boundary(end) {
        end -= 1;
    }
    &username[..end]
}

/// レートリミッタのキーを作る。[`normalize_username`] を通すことで、`Kyoko` と `kyoko` が
/// 別バケットに分かれて制限を回避できてしまうのを防ぐ (照合側は正規化済みのため、
/// 大文字小文字を変えただけの試行も同じアカウントへの試行になる)。
fn user_key(username: &str) -> Key {
    Key::User(truncate_username(&normalize_username(username)).to_string())
}

/// キー (ユーザー名・接続元など、呼び出し側が決める) 単位で試行回数を制限する、プロセス内 in-memory なレートリミッタ。
/// Argon2 の検証コストはリクエストごとに発生するため、制限が無いとパスワード総当たりだけでなく
/// CPU 枯渇 DoS の余地もある。ログインとパスワード変更で別々の実体を持たせ、片方への試行が
/// もう片方を巻き込んで止めないようにする。
///
/// キー単位の制限だけでは、攻撃者が毎回異なるキー (ユーザー名・接続元) で送ることで際限なく
/// Argon2 を実行させられる上、Map のキーも無制限に増え続けてしまう。これを防ぐため、以下の3つを
/// 組み合わせている。
///
/// - 全リクエスト共通のグローバル上限も別途課す
/// - 追跡する distinct なキーの数に上限を設け、超過時は新しいキーを拒否する
/// - 呼び出しのたびに期限切れの試行を掃除し、空になったキーは Map から削除する
///
/// サーバー1台の小規模な運用を想定した簡易実装であり、プロセス再起動や
/// マルチインスタンス運用をまたいだ制限は行わない。
///
/// キーに何を使うかは呼び出し側が決める。ログインでユーザー名だけをキーにすると、第三者が
/// 本人の名前で試し続けるだけで別の端末の本人を締め出せ、全体の上限が低いと無関係な全員も
/// 締め出せる (認証可用性への DoS)。そのためログインでは「接続元 + ユーザー名」をキーにした
/// 実体と接続元をキーにした実体を組み合わせ、攻撃者と同じ接続元だけが止まるようにする
/// (接続元は `forwarded::rate_limit_key` で、IPv6 は /64 ごと)。
/// 全体の上限は CPU 枯渇 DoS の歯止めとして緩めに取る ([`Self::for_logins`]、docs/authentication.md)。
///
/// パスワード変更に使う実体では、キーになるユーザー名をセッションから取るため、攻撃者が
/// 名前を変えて回る経路は無い。グローバル上限は distinct 名への対策としてではなく、
/// 複数のセッションを奪われた場合に Argon2 の総量を頭打ちにするために効く。
pub struct AttemptRateLimiter {
    max_attempts: u32,
    global_max_attempts: u32,
    max_tracked_names: usize,
    window: Duration,
    attempts: Mutex<HashMap<Key, Vec<(u64, Instant)>>>,
    /// [`Self::try_acquire`] が発行する予約 ID の採番用カウンタ。
    next_id: AtomicU64,
}

#[derive(Hash, Eq, PartialEq, Clone)]
enum Key {
    /// 呼び出し側のキーに依存しない、全体の試行回数を数えるためのキー。
    Global,
    User(String),
}

/// [`AttemptRateLimiter::try_acquire`] が返す、記録した試行1件の識別子。
/// [`AttemptRateLimiter::release`] にそのまま渡すことで、他の並行リクエストの記録を
/// 誤って取り消さずに、この呼び出しが記録した分だけを正確に取り消せる。
#[derive(Debug, Clone, Copy)]
pub struct Reservation(u64);

impl AttemptRateLimiter {
    /// 既定値: 同一キーにつき 1 分間に 5 回まで、全体で 1 分間に 30 回まで。
    pub fn with_defaults() -> Self {
        Self::new(5, 30, Duration::from_secs(60))
    }

    /// ログインの「接続元 + ユーザー名」単位 (接続元は `forwarded::rate_limit_key`、IPv6 は /64。
    /// 接続元が分からなければユーザー名単位)。
    /// 同じ組につき1分に5回まで、全体で1分に300回まで。
    ///
    /// ADR: 全体の上限は、接続元を変えながら Argon2 を回させ続ける CPU 枯渇 DoS の歯止め。
    /// 低いと第三者が無関係な全員を締め出せるため、ユーザー名単位だった頃 (30回) より緩める。
    pub fn for_logins() -> Self {
        Self::new(5, 300, Duration::from_secs(60))
    }

    /// ログインの接続元単位 (`forwarded::rate_limit_key`、IPv6 は /64)。同じ接続元から1分に30回まで
    /// (名前を変えながらの試行を止める)。
    /// 全体の上限は [`Self::for_logins`] の実体で課すため、ここでは設けない。
    pub fn for_login_clients() -> Self {
        Self::new(30, u32::MAX, Duration::from_secs(60))
    }

    /// アプリの LINE・Apple ログインの `nonce` の発行 (docs/mobile-app.md) 向け。キーは接続元
    /// (`forwarded::rate_limit_key`、IPv6 は /64)。同じ接続元から1分に10回まで。
    ///
    /// ADR: 1回のログインで発行は1回なので、やり直しを重ねても届かない値にする。全体の上限は
    /// 設けない (一人がそれを使い切って、ほかの人のログインを止められないようにする)。持てる数の
    /// 上限は `NonceStore` が課す。
    pub fn for_app_login_nonces() -> Self {
        Self::new(10, u32::MAX, Duration::from_secs(60))
    }

    /// メールを送らせる要求 (サインアップ・パスワードリセット) 向け。キーは接続元
    /// (`forwarded::rate_limit_key`、IPv6 は /64)。同じ接続元から10分に5回まで、全体で10分に50回まで。
    ///
    /// ADR: ログインより窓を長く取る。1回ごとに実際にメールが出るため、他人のアドレスへの
    /// 大量送信 (SMTP サービスの送信枠を使い切る・迷惑メール扱いされる) を抑える方を優先する。
    pub fn for_mail_requests() -> Self {
        Self::new(5, 50, Duration::from_secs(600))
    }

    /// アプリ内課金の取引の確かめ (`POST /payments/apple/transactions`) 向け。キーはユーザー ID。
    /// 同じユーザーから1分に10回まで、全体で1分に300回まで。
    ///
    /// ADR: 1回ごとに App Store Server API を最大2回呼ぶ。乱打で鍵ごとの上限に当たり、本物の購入の
    /// 確かめや通知の取り直しが 429 で失敗しないようにする。起動時の送り直しは終えていない取引の数だけで、
    /// ふつうは数件なので届かない。
    pub fn for_app_store_transactions() -> Self {
        Self::new(10, 300, Duration::from_secs(60))
    }

    pub fn new(max_attempts: u32, global_max_attempts: u32, window: Duration) -> Self {
        Self {
            max_attempts,
            global_max_attempts,
            max_tracked_names: 10_000,
            window,
            attempts: Mutex::new(HashMap::new()),
            next_id: AtomicU64::new(0),
        }
    }

    /// `key` について、直近 `window` 以内の試行回数がキー単位・全体単位どちらの
    /// 上限も未満なら、この呼び出し自体を1回の試行として記録し [`Reservation`] を返す。
    /// 上限に達している場合は記録せず `None` を返す。
    ///
    /// 認証成功時はこの呼び出しで消費した分を [`Self::release`] で解放すること
    /// (でないと、正しいパスワードで何度もログインし直しただけの正当な利用者も
    /// ロックされ得る)。
    pub fn try_acquire(&self, key: &str) -> Option<Reservation> {
        let now = Instant::now();
        let user_key = user_key(key);

        // 別リクエストのロック中に panic した場合でも制限機能自体は継続させたいので、
        // poison した場合は中身を引き継いで使う。
        let mut attempts = self.attempts.lock().unwrap_or_else(|e| e.into_inner());

        // 期限切れの試行を掃除し、空になったキーは Map から削除する。
        // これにより distinct なキーを送り続けられても Map が際限なく肥大化しないようにする。
        attempts.retain(|_, v| {
            v.retain(|(_, t)| now.duration_since(*t) < self.window);
            !v.is_empty()
        });

        // 全体の試行回数がグローバル上限に達していれば、キーに関わらず拒否する
        // (異なるキーを使った CPU 枯渇 DoS 対策)。
        if attempts.entry(Key::Global).or_default().len() as u32 >= self.global_max_attempts {
            return None;
        }

        // 追跡中の distinct なキーの数が上限に達しており、かつこのキーが未追跡なら、
        // Map をこれ以上増やさないため拒否する (直前で必ず作られる Key::Global の分を1件除いて数える)。
        let tracked_user_count = attempts.len().saturating_sub(1);
        if !attempts.contains_key(&user_key) && tracked_user_count >= self.max_tracked_names {
            return None;
        }

        if attempts.entry(user_key.clone()).or_default().len() as u32 >= self.max_attempts {
            return None;
        }

        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        attempts.entry(Key::Global).or_default().push((id, now));
        attempts.entry(user_key).or_default().push((id, now));
        Some(Reservation(id))
    }

    /// [`Self::try_acquire`] が返した [`Reservation`] に対応する試行1件を取り消す
    /// (キー単位・グローバル単位それぞれ1件ずつ)。認証成功時に呼び、正しいパスワード
    /// でのログインが上限消費に数えられないようにする。`key` は取得時と同じものを渡す。
    ///
    /// 予約 ID で一致する要素だけを取り除くため、並行リクエストが同時に走っていても
    /// 他のリクエストの記録を誤って取り消すことはない。
    pub fn release(&self, key: &str, reservation: Reservation) {
        let user_key = user_key(key);
        let mut attempts = self.attempts.lock().unwrap_or_else(|e| e.into_inner());

        for key in [Key::Global, user_key] {
            if let Some(v) = attempts.get_mut(&key) {
                v.retain(|(id, _)| *id != reservation.0);
                if v.is_empty() {
                    attempts.remove(&key);
                }
            }
        }
    }
}

/// ログイン済みの利用者であり、かつ DB 上にそのユーザーが実在することを要求する axum extractor。
/// 保護したいハンドラの引数に加えるだけで使える。
///
/// `Authorization: Bearer` があればモバイルアプリのトークン (`crate::app_token`) で、無ければ
/// セッションで判定する (docs/mobile-app.md)。両方あるときはトークンだけを見る。
pub struct AuthUser {
    pub id: i64,
    pub username: String,
    pub role: Role,
    /// アプリのトークンで来たときの `app_tokens.id`。セッションで来たときは `None`。
    pub app_token_id: Option<i64>,
}

/// ログインの証しの出どころ。無効と分かったときに、その証しを破棄するために持つ。
enum Credential {
    Session(Session),
    AppToken { id: i64, session_generation: i64 },
}

impl Credential {
    async fn session_generation(&self) -> Result<Option<i64>, Error> {
        match self {
            Credential::Session(session) => session
                .get(SESSION_GENERATION_KEY)
                .await
                .map_err(Error::Session),
            Credential::AppToken {
                session_generation, ..
            } => Ok(Some(*session_generation)),
        }
    }

    async fn discard(&self, pool: &SqlitePool) -> Result<(), AppError> {
        match self {
            Credential::Session(session) => session.flush().await.map_err(Error::Session)?,
            Credential::AppToken {
                id,
                session_generation,
            } => crate::app_token::discard(pool, *id, *session_generation).await?,
        }
        Ok(())
    }
}

impl FromRequestParts<AppState> for AuthUser {
    type Rejection = AppError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let (user_id, credential) = match crate::app_token::bearer(&parts.headers) {
            Some(token) => {
                let found = crate::app_token::find(&state.pool, token, state.session_expiry_days)
                    .await?
                    .ok_or(AppError::Unauthorized)?;
                // 最終利用の記録は失効の起算に使う。記録できなくても操作は通す。
                if found.needs_touch
                    && let Err(err) = crate::app_token::touch(&state.pool, found.id).await
                {
                    tracing::warn!(error = %crate::error_chain_line(&err), user_id = found.user_id, "アプリのトークンの最終利用を記録できませんでした");
                }
                (
                    found.user_id,
                    Credential::AppToken {
                        id: found.id,
                        session_generation: found.session_generation,
                    },
                )
            }
            None => {
                let session = Session::from_request_parts(parts, state)
                    .await
                    .map_err(|(_, msg)| AppError::Internal(Error::MissingSessionLayer(msg)))?;
                let user_id: i64 = session
                    .get(SESSION_USER_ID_KEY)
                    .await
                    .map_err(Error::Session)?
                    .ok_or(AppError::Unauthorized)?;
                (user_id, Credential::Session(session))
            }
        };

        // セッションは有効でも、DB 上のユーザーが削除済みの可能性がある。ここで存在確認して
        // おかないと、AuthUser だけに依存するハンドラが削除済みユーザーのセッションを
        // 認証済みとして扱ってしまう。
        let row = sqlx::query!(
            r#"SELECT id AS "id!: i64", username, role AS "role: Role", frozen,
                      session_generation AS "session_generation!: i64",
                      (COALESCE(last_seen_at, created_at)
                           < strftime('%Y-%m-%dT%H:%M:%fZ', 'now', '-1 day')
                       OR deletion_origin = 'inactive') AS "needs_touch!: bool"
               FROM users WHERE id = ?"#,
            user_id
        )
        .fetch_optional(&state.pool)
        .await?;

        let Some(row) = row else {
            // ユーザーが削除された後の残留セッション。ここで破棄しておく。
            credential.discard(&state.pool).await?;
            return Err(AppError::Unauthorized);
        };

        // 凍結はセッション継続も断つ (docs/authentication.md)。破棄してログイン画面へ送れば、
        // ログインし直したときに凍結中であることを本人に伝えられる。
        if row.frozen != 0 {
            credential.discard(&state.pool).await?;
            return Err(AppError::Unauthorized);
        }

        // パスワード変更で世代が上がると、変更した端末以外のセッションはここで断たれる
        // (docs/authentication.md)。
        let generation = credential.session_generation().await?;
        if !session_generation_is_current(generation, row.session_generation) {
            credential.discard(&state.pool).await?;
            return Err(AppError::Unauthorized);
        }

        // 使われていることを記録する (放置アカウントの自動退会の起算、docs/authentication.md)。
        // 記録できなくても操作は通す。読むだけのリクエストが書き込みの競合に巻き込まれないよう、
        // 更新が要るときだけ書く。
        if row.needs_touch
            && let Err(err) = crate::account::touch_last_seen(&state.pool, row.id).await
        {
            tracing::warn!(error = %crate::error_chain_line(&err), user_id = row.id, "最終アクセスを記録できませんでした");
        }

        Ok(AuthUser {
            id: row.id,
            username: row.username,
            role: row.role,
            app_token_id: match credential {
                Credential::AppToken { id, .. } => Some(id),
                Credential::Session(_) => None,
            },
        })
    }
}

/// セッションが持つ世代が、DB 側の現在の世代 (`users.session_generation`) と一致するか。
///
/// 世代を持たないセッションは 0 として扱う。この仕組みより前に作られたセッションを、まだ
/// パスワードを変えていないユーザー (世代 0) に限って通すための移行措置。パスワードを
/// 変えた後の世代 (1 以上) とは一致しないため、無効化をすり抜けることはない。
///
/// 変更したセッションと同じ Cookie のリクエストが、世代の書き戻しと同時に走ると、古い世代を
/// 読んで自分自身を断つことがある。結果は入り直しで済むため、窓を閉じるためにセッションの
/// 書き込みを直列化する仕組みは入れない。
fn session_generation_is_current(session_generation: Option<i64>, current: i64) -> bool {
    session_generation.unwrap_or(0) == current
}

/// [`AuthUser`] に加えて `role = 'admin'` を要求する axum extractor (docs/authentication.md)。
/// 管理者APIのハンドラの引数に加えるだけで、一般ユーザーからの呼び出しを 403 で弾ける。
pub struct AdminUser(pub AuthUser);

impl FromRequestParts<AppState> for AdminUser {
    type Rejection = AppError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let user = AuthUser::from_request_parts(parts, state).await?;
        if user.role != Role::Admin {
            return Err(AppError::Forbidden);
        }
        Ok(AdminUser(user))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::insert_user_with_password;

    #[test]
    fn hash_then_verify_roundtrip() {
        let hash = hash_password("correct horse battery staple").expect("failed to hash password");
        assert!(hash.starts_with("$argon2id$"));
        assert!(
            verify_password("correct horse battery staple", &hash)
                .expect("verify_password should not error")
        );
        assert!(
            !verify_password("wrong password", &hash).expect("verify_password should not error")
        );
    }

    #[test]
    fn verify_against_garbage_hash_is_error() {
        let result = verify_password("anything", "not a valid phc string");
        assert!(matches!(result, Err(Error::Verify(_))));
    }

    #[test]
    fn dummy_hash_uses_same_parameters_as_real_hashes() {
        let real = hash_password("x").expect("failed to hash password");
        // "$argon2id$v=19$m=...,t=...,p=..." までが一致していればコストが揃っている。
        let params = |h: &str| h.splitn(5, '$').take(4).collect::<Vec<_>>().join("$");
        assert_eq!(params(&real), params(&DUMMY_HASH));
    }

    fn policy(min_length: usize, required_classes: &[CharClass]) -> PasswordConfig {
        PasswordConfig {
            min_length,
            required_classes: required_classes.to_vec(),
        }
    }

    #[test]
    fn validate_password_requires_eight_characters_by_default() {
        assert_eq!(
            validate_password(&PasswordConfig::default(), "1234"),
            Err(PasswordPolicyError::TooShort { min_length: 8 })
        );
        assert_eq!(
            validate_password(&PasswordConfig::default(), "12345678"),
            Ok(())
        );
    }

    #[test]
    fn validate_password_rejects_a_password_shorter_than_the_minimum() {
        assert_eq!(
            validate_password(&policy(8, &[]), "1234567"),
            Err(PasswordPolicyError::TooShort { min_length: 8 })
        );
    }

    #[test]
    fn validate_password_counts_multibyte_characters_as_one_each() {
        assert_eq!(validate_password(&policy(4, &[]), "ぱすわーど"), Ok(()));
        assert_eq!(
            validate_password(&policy(4, &[]), "ぱすわ"),
            Err(PasswordPolicyError::TooShort { min_length: 4 })
        );
    }

    #[test]
    fn validate_password_requires_every_configured_class() {
        let config = policy(4, &[CharClass::Digit, CharClass::Uppercase]);
        assert_eq!(validate_password(&config, "Abc1"), Ok(()));
        assert_eq!(
            validate_password(&config, "abcd"),
            Err(PasswordPolicyError::MissingClasses {
                missing: vec![CharClass::Digit, CharClass::Uppercase],
            })
        );
    }

    /// 種類名と合うよう ASCII で判定する (ひらがなが「英小文字」に該当しない)。
    #[test]
    fn validate_password_classifies_only_ascii_letters_as_letter_classes() {
        assert_eq!(
            validate_password(&policy(4, &[CharClass::Lowercase]), "ぱすわーど"),
            Err(PasswordPolicyError::MissingClasses {
                missing: vec![CharClass::Lowercase],
            })
        );
        assert_eq!(
            validate_password(&policy(4, &[CharClass::Uppercase]), "привет"),
            Err(PasswordPolicyError::MissingClasses {
                missing: vec![CharClass::Uppercase],
            })
        );
    }

    /// ASCII の英数字以外はすべて記号として数える (日本語・絵文字を含む)。
    #[test]
    fn validate_password_counts_non_ascii_characters_as_symbols() {
        let config = policy(4, &[CharClass::Symbol]);
        assert_eq!(validate_password(&config, "ぱすわーど"), Ok(()));
        assert_eq!(validate_password(&config, "pass🔒"), Ok(()));
    }

    #[test]
    fn validate_password_treats_non_alphanumeric_characters_as_symbols() {
        let config = policy(4, &[CharClass::Symbol]);
        assert_eq!(validate_password(&config, "abc!"), Ok(()));
        assert_eq!(
            validate_password(&config, "abc1"),
            Err(PasswordPolicyError::MissingClasses {
                missing: vec![CharClass::Symbol],
            })
        );
    }

    /// 設定を直せるように、足りない文字種をすべて挙げる。
    #[test]
    fn password_policy_error_lists_every_missing_class() {
        let error = PasswordPolicyError::MissingClasses {
            missing: vec![CharClass::Digit, CharClass::Symbol],
        };
        assert_eq!(
            error.to_string(),
            "パスワードには数字・記号を含めてください"
        );
    }

    #[sqlx::test]
    async fn authenticate_succeeds_with_correct_password(pool: SqlitePool) {
        insert_user_with_password(&pool, "admin", "password").await;

        let user = authenticate(&pool, "admin", "password")
            .await
            .expect("authenticate should not error")
            .expect("correct credentials should authenticate");
        assert_eq!(user.username, "admin");
    }

    #[sqlx::test]
    async fn authenticate_fails_with_wrong_password(pool: SqlitePool) {
        insert_user_with_password(&pool, "admin", "password").await;

        let user = authenticate(&pool, "admin", "wrong-password")
            .await
            .expect("authenticate should not error");
        assert!(user.is_none());
    }

    #[sqlx::test]
    async fn authenticate_fails_for_unknown_user(pool: SqlitePool) {
        let user = authenticate(&pool, "no-such-user", "password")
            .await
            .expect("authenticate should not error");
        assert!(user.is_none());
    }

    #[sqlx::test]
    async fn authenticate_reports_corrupt_hash_as_error(pool: SqlitePool) {
        sqlx::query!(
            "INSERT INTO users (username, password_hash, email_verified) VALUES (?, ?, 1)",
            "broken",
            "not a valid phc string"
        )
        .execute(&pool)
        .await
        .expect("failed to insert test user");

        let result = authenticate(&pool, "broken", "password").await;
        assert!(matches!(result, Err(AppError::Internal(Error::Verify(_)))));
    }

    #[sqlx::test]
    async fn create_user_stores_username_in_lowercase(pool: SqlitePool) {
        let hash = hash_password("password").expect("failed to hash password");
        create_user(&pool, "Foo", NewPassword::Usable(&hash), Role::User)
            .await
            .expect("failed to create test user");

        let stored: String =
            sqlx::query_scalar!("SELECT username FROM users WHERE password_hash = ?", hash)
                .fetch_one(&pool)
                .await
                .expect("failed to fetch stored username");
        assert_eq!(stored, "foo");
    }

    /// `--create-user --admin` で指定した権限が保存され、同じ権限として読み戻せること。
    #[sqlx::test]
    async fn create_user_stores_given_role(pool: SqlitePool) {
        let hash = hash_password("password").expect("failed to hash password");
        for (username, role) in [("boss", Role::Admin), ("member", Role::User)] {
            create_user(&pool, username, NewPassword::Usable(&hash), role)
                .await
                .expect("failed to create test user");
        }

        for (username, expected) in [("boss", Role::Admin), ("member", Role::User)] {
            let stored = sqlx::query_scalar!(
                r#"SELECT role AS "role: Role" FROM users WHERE username = ?"#,
                username
            )
            .fetch_one(&pool)
            .await
            .expect("failed to fetch stored role");
            assert_eq!(stored, expected);
        }
    }

    /// 大文字を含む名前で作成したユーザーが、どの大文字小文字でもログインできること
    /// (作成時と照合時が同じ正規化関数を通っていることの確認)。
    #[sqlx::test]
    async fn authenticate_is_case_insensitive_for_username(pool: SqlitePool) {
        insert_user_with_password(&pool, "Foo", "password").await;

        for attempt in ["foo", "Foo", "FOO"] {
            let user = authenticate(&pool, attempt, "password")
                .await
                .expect("authenticate should not error")
                .unwrap_or_else(|| panic!("'{attempt}' でログインできるべき"));
            assert_eq!(user.username, "foo");
        }
    }

    #[sqlx::test]
    async fn create_user_rejects_username_differing_only_in_case(pool: SqlitePool) {
        let hash = hash_password("password").expect("failed to hash password");
        create_user(&pool, "foo", NewPassword::Usable(&hash), Role::User)
            .await
            .expect("failed to create test user");

        let err = create_user(&pool, "FOO", NewPassword::Usable(&hash), Role::User)
            .await
            .expect_err("大文字違いの同名ユーザーは UNIQUE 違反になるべき");
        assert!(matches!(err, sqlx::Error::Database(ref e) if e.is_unique_violation()));
    }
}

#[cfg(test)]
mod rate_limiter_tests {
    use super::*;

    #[test]
    fn allows_up_to_max_attempts_then_blocks() {
        let limiter = AttemptRateLimiter::new(3, 100, Duration::from_secs(60));

        assert!(limiter.try_acquire("admin").is_some());
        assert!(limiter.try_acquire("admin").is_some());
        assert!(limiter.try_acquire("admin").is_some());
        assert!(limiter.try_acquire("admin").is_none());
    }

    #[test]
    fn tracks_each_username_independently() {
        let limiter = AttemptRateLimiter::new(1, 100, Duration::from_secs(60));

        assert!(limiter.try_acquire("admin").is_some());
        assert!(limiter.try_acquire("admin").is_none());
        assert!(limiter.try_acquire("other").is_some());
    }

    /// 大文字小文字違いは同一バケットとして数える。分かれてしまうと、名前の綴りを
    /// 変えるだけでユーザー単位の上限を回避できてしまう。
    #[test]
    fn treats_usernames_case_insensitively() {
        let limiter = AttemptRateLimiter::new(1, 100, Duration::from_secs(60));

        assert!(limiter.try_acquire("Kyoko").is_some());
        assert!(limiter.try_acquire("kyoko").is_none());
        assert!(limiter.try_acquire("KYOKO").is_none());
    }

    #[test]
    fn release_matches_username_case_insensitively() {
        let limiter = AttemptRateLimiter::new(1, 100, Duration::from_secs(60));

        let reservation = limiter
            .try_acquire("kyoko")
            .expect("try_acquire should succeed while under the limit");
        assert!(limiter.try_acquire("kyoko").is_none());

        limiter.release("KYOKO", reservation);
        assert!(limiter.try_acquire("Kyoko").is_some());
    }

    #[test]
    fn global_limit_blocks_even_with_distinct_usernames() {
        let limiter = AttemptRateLimiter::new(100, 3, Duration::from_secs(60));

        assert!(limiter.try_acquire("user-1").is_some());
        assert!(limiter.try_acquire("user-2").is_some());
        assert!(limiter.try_acquire("user-3").is_some());
        // 別のユーザー名でも、グローバル上限を超えていれば拒否される。
        assert!(limiter.try_acquire("user-4").is_none());
    }

    #[test]
    fn does_not_track_more_than_max_names() {
        let limiter = AttemptRateLimiter {
            max_tracked_names: 2,
            ..AttemptRateLimiter::new(100, 100, Duration::from_secs(60))
        };

        assert!(limiter.try_acquire("user-1").is_some());
        assert!(limiter.try_acquire("user-2").is_some());
        // 追跡上限に達しているため、3件目の新規名は拒否される。
        assert!(limiter.try_acquire("user-3").is_none());
        // 既に追跡中の名前は引き続き許可される。
        assert!(limiter.try_acquire("user-1").is_some());
    }

    #[test]
    fn truncates_long_usernames_on_char_boundary() {
        let long = "あ".repeat(200); // 600 バイト
        let truncated = truncate_username(&long);
        assert!(truncated.len() <= MAX_TRACKED_USERNAME_LEN);
        assert!(truncated.chars().all(|c| c == 'あ'));
    }

    #[test]
    fn release_lets_successful_login_retry_without_being_blocked() {
        let limiter = AttemptRateLimiter::new(1, 100, Duration::from_secs(60));

        let reservation = limiter
            .try_acquire("admin")
            .expect("try_acquire should succeed while under the limit");
        // 解放せずに再試行すると、ユーザー単位の上限 (1回) に達して拒否される。
        assert!(limiter.try_acquire("admin").is_none());

        limiter.release("admin", reservation);
        // 解放後は同じユーザー名でまた1回試行できる (認証成功時の挙動を模している)。
        assert!(limiter.try_acquire("admin").is_some());
    }

    #[test]
    fn release_also_frees_one_global_slot() {
        let limiter = AttemptRateLimiter::new(100, 1, Duration::from_secs(60));

        let reservation = limiter
            .try_acquire("user-1")
            .expect("try_acquire should succeed while under the limit");
        // グローバル上限 (1回) に達しているため、別ユーザーでも拒否される。
        assert!(limiter.try_acquire("user-2").is_none());

        limiter.release("user-1", reservation);
        // グローバル分も解放されるため、別ユーザーの試行が通るようになる。
        assert!(limiter.try_acquire("user-2").is_some());
    }

    #[test]
    fn release_only_removes_the_reservation_it_was_given() {
        // 認証成功リクエスト (admin) の release() が、その後に届いた別リクエスト
        // (attacker) の記録を誤って取り消さないことを確認する (取り違えると、並行
        // リクエストを送るだけでグローバル上限を継続的にすり抜けられてしまう)。
        let limiter = AttemptRateLimiter::new(100, 2, Duration::from_secs(60));

        // Global: [admin]
        let admin_reservation = limiter
            .try_acquire("admin")
            .expect("try_acquire should succeed while under the limit");
        // admin の認証処理中に、別ユーザーの試行が (グローバルキューの) 後続として記録される。
        limiter
            .try_acquire("attacker")
            .expect("try_acquire should succeed while under the limit"); // Global: [admin, attacker] (上限 2 に到達)

        limiter.release("admin", admin_reservation);

        // admin 分だけがグローバル上限から取り除かれ、attacker の記録は残っているはず
        // (末尾を pop する実装だと attacker の記録が誤って消え、下の2回が両方通ってしまう)。
        assert!(limiter.try_acquire("someone-else").is_some()); // Global: [attacker, someone-else]
        assert!(
            limiter.try_acquire("another").is_none(),
            "attacker の試行がグローバルキューに残っているべき"
        );
    }

    #[test]
    fn release_with_unknown_reservation_is_a_no_op() {
        let limiter = AttemptRateLimiter::with_defaults();

        // 対応する試行が存在しない Reservation で解放してもパニックしない。
        limiter.release("never-tried", Reservation(u64::MAX));
    }

    #[test]
    fn session_generation_must_match_the_current_one() {
        assert!(session_generation_is_current(Some(3), 3));
        assert!(!session_generation_is_current(Some(2), 3));
        assert!(!session_generation_is_current(Some(4), 3));
    }

    /// 世代を持たない (この仕組みより前に作られた) セッションは、まだパスワードを変えて
    /// いないユーザーに限って通す。
    #[test]
    fn session_without_a_generation_is_current_only_before_the_first_password_change() {
        assert!(session_generation_is_current(None, 0));
        assert!(!session_generation_is_current(None, 1));
    }
}
