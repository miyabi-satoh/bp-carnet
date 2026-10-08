//! セッション Cookie の署名鍵の解決と `SessionManagerLayer` の組み立て。

use std::io::Write;
use std::path::{Path, PathBuf};

use axum::extract::Request;
use axum::http::{HeaderValue, header};
use axum::middleware::Next;
use axum::response::Response;
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use tower_sessions::cookie::Cookie;
use tower_sessions::service::SignedCookie;
use tower_sessions::{Expiry, SessionManagerLayer, cookie::Key};
use tower_sessions_sqlx_store::SqliteStore;

use crate::config::{self, SessionConfig};

const KEY_LEN: usize = 64;

pub type SessionLayer = SessionManagerLayer<SqliteStore, SignedCookie>;

/// セッション Cookie の名前。
///
/// Secure を付けるとき (https で公開するとき) は `__Host-` を付け、ブラウザに `Path=/`・`Domain` 無しを守らせる。
/// 同じ登録ドメインの別のサブドメイン (ブログなど) が乗っ取られても、`Domain` 付きの同名 Cookie を
/// 送り込んでセッションを差し替える (Cookie tossing) ことができないようにするため。
/// `__Host-` は Secure が必須なので、`secure_cookie` が無効 (http) のときは付けない。
pub fn cookie_name(secure: bool) -> &'static str {
    if secure { "__Host-id" } else { "id" }
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("session.secret のデコードに失敗しました (base64 である必要があります)")]
    Decode(#[source] base64::DecodeError),
    #[error("セッション鍵の長さが不正です ({0} バイト。64 バイト必要です)")]
    InvalidLength(usize),
    #[error("セッション鍵ファイルの読み書きに失敗しました: {path}")]
    KeyFile {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
}

/// セッションの署名鍵を解決する。
///
/// - `config.secret` が空でなければ、base64 デコードしてそのまま使う (64 バイト必須)。
/// - 空なら `key_path` を読み書きして自動生成・永続化する (初回起動時に無設定でも動くように)。
///   永続化されたランダム 64 バイトの鍵は運用者が手で用意する secret より弱くないため、
///   bind 先に関わらず自動生成で良い。`secret` は複数インスタンスで鍵を共有したい場合などの
///   オプションとする。
pub fn resolve_key(config: &SessionConfig, key_path: &Path) -> Result<Key, Error> {
    if !config.secret.is_empty() {
        let bytes = STANDARD.decode(&config.secret).map_err(Error::Decode)?;
        if bytes.len() != KEY_LEN {
            return Err(Error::InvalidLength(bytes.len()));
        }
        return Ok(Key::from(&bytes));
    }

    load_or_generate_key(key_path)
}

fn load_or_generate_key(path: &Path) -> Result<Key, Error> {
    match std::fs::read(path) {
        Ok(bytes) if bytes.len() == KEY_LEN => Ok(Key::from(&bytes)),
        Ok(bytes) => Err(Error::InvalidLength(bytes.len())),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => generate_and_persist_key(path),
        Err(source) => Err(Error::KeyFile {
            path: path.to_path_buf(),
            source,
        }),
    }
}

fn generate_and_persist_key(path: &Path) -> Result<Key, Error> {
    let key = Key::generate();

    config::create_owner_only_parent_dir(path).map_err(|source| Error::KeyFile {
        path: path.to_path_buf(),
        source,
    })?;

    // クラッシュ等で中途半端な (短い) ファイルが残ると、次回起動時に `Key::from` が
    // 長さ不足でパニックする恐れがあるため、同じディレクトリの一時ファイルに書いてから
    // rename する (rename は同一ファイルシステム上 atomic なため、書きかけの状態を晒さない)。
    let tmp_path = path.with_extension("tmp");
    let write_result: std::io::Result<()> = (|| {
        // 前回のクラッシュ等で一時ファイルが残っていると `create_new` が失敗するため、
        // 先に取り除いておく (単一インスタンス起動が前提)。
        let _ = std::fs::remove_file(&tmp_path);
        let mut file = create_owner_only_file(&tmp_path)?;
        file.write_all(key.master())?;
        file.sync_all()?;
        std::fs::rename(&tmp_path, path)?;
        Ok(())
    })();

    if let Err(source) = write_result {
        let _ = std::fs::remove_file(&tmp_path);
        return Err(Error::KeyFile {
            path: path.to_path_buf(),
            source,
        });
    }

    Ok(key)
}

/// 所有者のみ読み書き可能な状態でファイルを新規作成する。
///
/// `File::create` してから `chmod` する二段階の方式だと、作成直後の一瞬
/// umask 依存のパーミッションが晒される (group/other から読めてしまう恐れがある)
/// ため、作成時点から owner-only になるようオープンフラグで指定する。
#[cfg(unix)]
fn create_owner_only_file(path: &Path) -> std::io::Result<std::fs::File> {
    use std::os::unix::fs::OpenOptionsExt;
    std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)
}

#[cfg(not(unix))]
fn create_owner_only_file(path: &Path) -> std::io::Result<std::fs::File> {
    // non-unix では owner-only な権限を保証できない (ACL 未対応)。
    std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
}

/// `__Host-` の Cookie を消す `Set-Cookie` に Secure を付け直す。`SessionManagerLayer` の外側に掛ける。
///
/// tower-sessions はセッションを空にすると (ログアウト・無効なセッションの破棄)、リクエストから読んだ Cookie を
/// 元に削除用の Cookie を作り、パスとドメインしか付け直さない (`tower-sessions-0.14.0/src/service.rs`)。
/// Secure の無い `__Host-` の `Set-Cookie` はブラウザに捨てられ、Cookie が消えずに残るため。
pub async fn secure_host_prefixed_cookies(req: Request, next: Next) -> Response {
    let mut response = next.run(req).await;
    let set_cookies: Vec<HeaderValue> = response
        .headers()
        .get_all(header::SET_COOKIE)
        .iter()
        .cloned()
        .collect();
    if !set_cookies.iter().any(needs_secure) {
        return response;
    }
    let headers = response.headers_mut();
    headers.remove(header::SET_COOKIE);
    for value in set_cookies {
        headers.append(header::SET_COOKIE, with_secure_if_needed(value));
    }
    response
}

fn needs_secure(value: &HeaderValue) -> bool {
    parse_set_cookie(value)
        .is_some_and(|cookie| cookie.name().starts_with("__Host-") && cookie.secure() != Some(true))
}

fn parse_set_cookie(value: &HeaderValue) -> Option<Cookie<'static>> {
    Cookie::parse(value.to_str().ok()?.to_string()).ok()
}

fn with_secure_if_needed(value: HeaderValue) -> HeaderValue {
    if !needs_secure(&value) {
        return value;
    }
    let Some(mut cookie) = parse_set_cookie(&value) else {
        return value;
    };
    cookie.set_secure(true);
    HeaderValue::from_str(&cookie.to_string()).unwrap_or(value)
}

/// axum に載せる `SessionManagerLayer` を組み立てる。
///
/// `with_always_save` が無いと `/me` のような読み取りだけのアクセスではセッションが
/// 「更新」されず、`OnInactivity` でも実質「ログインから N 日」にしかならないため付与する。
pub fn layer(store: SqliteStore, key: Key, config: &SessionConfig) -> SessionLayer {
    SessionManagerLayer::new(store)
        .with_name(cookie_name(config.secure_cookie))
        .with_secure(config.secure_cookie)
        .with_expiry(Expiry::OnInactivity(time::Duration::days(
            config.expiry_days,
        )))
        .with_always_save(true)
        .with_signed(key)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::TempDir;

    fn config_with_secret(secret: &str) -> SessionConfig {
        SessionConfig {
            secret: secret.to_string(),
            ..Default::default()
        }
    }

    /// 鍵ファイルを置く一時ディレクトリを作り、鍵ファイルのパスと一緒に返す。
    fn key_dir() -> (TempDir, PathBuf) {
        let tmp = TempDir::new("session");
        std::fs::create_dir_all(tmp.path()).expect("failed to create the temp dir");
        let key_path = tmp.path().join("session.key");
        (tmp, key_path)
    }

    #[test]
    fn generates_and_persists_key_on_first_call() {
        let (_tmp, key_path) = key_dir();
        let config = SessionConfig::default();

        let key1 = resolve_key(&config, &key_path).expect("first call should succeed");
        assert!(key_path.exists());
        assert_eq!(
            std::fs::read(&key_path)
                .expect("failed to read the generated key file")
                .len(),
            KEY_LEN
        );

        let key2 = resolve_key(&config, &key_path).expect("second call should succeed");
        assert_eq!(key1.master(), key2.master(), "2回目は同じ鍵を返すべき");
    }

    #[test]
    fn explicit_secret_must_be_64_bytes() {
        let (_tmp, key_path) = key_dir();
        let short = STANDARD.encode([0u8; 32]);
        let config = config_with_secret(&short);

        let err = resolve_key(&config, &key_path).expect_err("short secret should be rejected");
        assert!(matches!(err, Error::InvalidLength(32)));
    }

    #[test]
    fn explicit_secret_decodes_when_valid() {
        let (_tmp, key_path) = key_dir();
        let encoded = STANDARD.encode([7u8; KEY_LEN]);
        let config = config_with_secret(&encoded);

        let key = resolve_key(&config, &key_path).expect("valid secret should decode");
        assert_eq!(key.master(), [7u8; KEY_LEN]);
        assert!(!key_path.exists(), "明示指定時は鍵ファイルを作らない");
    }

    #[test]
    fn invalid_base64_secret_is_error() {
        let (_tmp, key_path) = key_dir();
        let config = config_with_secret("not base64!");

        let err = resolve_key(&config, &key_path).expect_err("invalid base64 should be rejected");
        assert!(matches!(err, Error::Decode(_)));
    }

    #[cfg(unix)]
    #[test]
    fn generated_key_file_is_owner_only() {
        use std::os::unix::fs::PermissionsExt;

        let (_tmp, key_path) = key_dir();
        let config = SessionConfig::default();

        resolve_key(&config, &key_path).expect("key generation should succeed");

        let mode = std::fs::metadata(&key_path)
            .expect("failed to read the generated key file metadata")
            .permissions()
            .mode();
        // owner 分のビットは umask の設定次第で削られる可能性があるため固定値と
        // 比較せず、group/other への権限が一切無いことだけを検証する。
        assert_eq!(mode & 0o077, 0, "group/other から読み書きできてはいけない");
    }

    #[cfg(unix)]
    #[test]
    fn stale_tmp_file_does_not_block_key_generation() {
        let (_tmp, key_path) = key_dir();
        let config = SessionConfig::default();

        // 前回のクラッシュ等で一時ファイルが残っていても、生成処理が失敗しない
        // ことを確認する (create_new(true) 化に伴う回帰防止)。
        std::fs::write(key_path.with_extension("tmp"), b"stale")
            .expect("failed to write the stale temp file");

        let key = resolve_key(&config, &key_path).expect("key generation should succeed");
        assert_eq!(
            std::fs::read(&key_path)
                .expect("failed to read the generated key file")
                .len(),
            KEY_LEN,
            "残存する一時ファイルを退けて鍵ファイルを生成できるべき"
        );
        assert_eq!(key.master().len(), KEY_LEN);
    }
}
