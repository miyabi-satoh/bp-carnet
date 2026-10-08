//! 設定ファイル (`config.toml`) の読み込みと、設定・データを置くディレクトリの解決。

use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::{Path, PathBuf};

use directories::ProjectDirs;
use serde::Deserialize;

use crate::app_version::AppVersion;
pub use crate::forwarded::ProxyHeaders;
use crate::forwarded::TrustedProxy;

/// `directories::ProjectDirs::from` の3引数。
/// macOS: `com.amiiby.bp-carnet` / Windows: `amiiby\bp-carnet` / Linux: `bp-carnet` に解決される。
/// 一度リリースした後に変更すると、既存ユーザーの設定・DB ファイルの配置場所が変わってしまうため注意。
const APP_QUALIFIER: &str = "com";
const APP_ORGANIZATION: &str = "amiiby";
const APP_APPLICATION: &str = "bp-carnet";

/// 改名 (bp-tracker → bp-carnet) 前の DB ファイル名。本番の DB はこの名前のまま複製している
/// (deploy/cloud/litestream.yml) ため、新名の DB が無いときに限って使い続ける。
const LEGACY_DB_FILE_NAME: &str = "bp-tracker.db";

/// この環境変数が設定されていれば、設定・データ・ログをすべてそのディレクトリ直下に置く。
/// Docker / systemd のように HOME が無い、あるいは配置場所を固定したい運用向け。
pub const HOME_ENV: &str = "BP_CARNET_HOME";

/// 環境変数の値。未設定と空文字列はどちらも `None` にする (`.env` に `KEY=` と書いて無効にする書き方を許すため)。
pub fn non_empty_env(name: &str) -> Option<String> {
    std::env::var(name).ok().filter(|value| !value.is_empty())
}

const CONFIG_FILE_NAME: &str = "config.toml";
const DB_FILE_NAME: &str = "bp-carnet.db";
const SESSION_KEY_FILE_NAME: &str = "session.key";
const LOG_DIR_NAME: &str = "logs";
const LOCK_FILE_NAME: &str = "bp-carnet.lock";

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(
        "設定・データディレクトリを解決できませんでした (HOME が無い環境では {HOME_ENV} を設定してください)"
    )]
    NoHomeDir,
    #[error("設定ファイルの読み込みに失敗しました: {path}")]
    Read {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("設定ファイルの解析に失敗しました: {path}")]
    Parse {
        path: PathBuf,
        #[source]
        source: toml::de::Error,
    },
    #[error("設定ファイルに {0} がありません: {1}")]
    Missing(&'static str, PathBuf),
    #[error("環境変数 {name} の値が正しくありません: {message}")]
    Env { name: &'static str, message: String },
}

/// 設定ファイル・DB・セッション鍵・ログ・シングルインスタンスロックの置き場所。
#[derive(Debug, Clone)]
pub struct AppDirs {
    pub config_dir: PathBuf,
    pub data_dir: PathBuf,
}

impl AppDirs {
    /// 1. `BP_CARNET_HOME` が設定されていれば config_dir = data_dir = その値
    /// 2. それ以外は OS 標準のアプリデータディレクトリ (`ProjectDirs`)
    pub fn resolve() -> Result<Self, Error> {
        if let Some(home) = std::env::var_os(HOME_ENV).filter(|v| !v.is_empty()) {
            let home = PathBuf::from(home);
            return Ok(Self {
                config_dir: home.clone(),
                data_dir: home,
            });
        }

        let dirs = ProjectDirs::from(APP_QUALIFIER, APP_ORGANIZATION, APP_APPLICATION)
            .ok_or(Error::NoHomeDir)?;
        Ok(Self {
            config_dir: dirs.config_dir().to_path_buf(),
            data_dir: dirs.data_local_dir().to_path_buf(),
        })
    }

    pub fn config_path(&self) -> PathBuf {
        self.config_dir.join(CONFIG_FILE_NAME)
    }

    /// 新名の DB が無く、旧名 (`bp-tracker.db`) の DB があれば、旧名のほうを使う。
    pub fn db_path(&self) -> PathBuf {
        let path = self.data_dir.join(DB_FILE_NAME);
        let legacy = self.data_dir.join(LEGACY_DB_FILE_NAME);
        if !path.exists() && legacy.exists() {
            return legacy;
        }
        path
    }

    pub fn session_key_path(&self) -> PathBuf {
        self.data_dir.join(SESSION_KEY_FILE_NAME)
    }

    pub fn log_dir(&self) -> PathBuf {
        self.data_dir.join(LOG_DIR_NAME)
    }

    /// シングルインスタンス化 (`single_instance` モジュール) 用のロックファイルの場所。
    pub fn lock_path(&self) -> PathBuf {
        self.data_dir.join(LOCK_FILE_NAME)
    }
}

/// ディレクトリを作成し、Unix では所有者のみ読み書き・実行可能 (0700) にする。
/// DB・セッション鍵・ログはいずれも個人の血圧記録という機微な情報を含むため、
/// 親ディレクトリ自体も group/other から読めないようにする。
///
/// 新規作成時は `DirBuilder::mode` で mkdir(2) 自体に 0700 を指定するため、
/// 作成直後の一瞬だけ umask 依存の緩い権限が晒される、という時間差を作らない
/// (`create_dir_all` してから `set_permissions` で上書きする方式だと、その間
/// group/other から読めてしまう恐れがある)。既に存在するディレクトリは
/// (`DirBuilder` が権限を変更しないため) `set_permissions` で明示的に矯正する。
#[cfg(unix)]
pub fn create_owner_only_dir(path: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::{DirBuilderExt, PermissionsExt};

    if path.is_dir() {
        return std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700));
    }

    std::fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(path)
}

#[cfg(not(unix))]
pub fn create_owner_only_dir(path: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(path)
}

/// `path` を置く親ディレクトリを [`create_owner_only_dir`] で用意する。
///
/// `path` がファイル名だけ (カレントディレクトリ相対) の場合、`parent()` は空文字列の
/// `Path` を返す。空パスに対する作成はエラーになるため、その場合は何もしない。
pub fn create_owner_only_parent_dir(path: &Path) -> std::io::Result<()> {
    match path.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => create_owner_only_dir(parent),
        _ => Ok(()),
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ServerConfig {
    pub bind: IpAddr,
    /// 0 (OS が空きポートを割り当てる) はサーバー用途では使えないため、
    /// 設定ファイル読み込み時点で弾く (`deserialize_nonzero_port` 参照)。
    #[serde(deserialize_with = "deserialize_nonzero_port")]
    pub port: u16,
    /// この一覧にあるアドレス・範囲から接続された場合に限り、プロキシのヘッダー (`proxy_headers`) を信頼する
    /// (`src/forwarded.rs`)。
    ///
    /// 既定は空 = どこから来ても信頼しない: 直接接続で信頼すると
    /// ヘッダーを名乗るだけで詐称できてしまう。
    pub trusted_proxies: Vec<TrustedProxy>,
    /// `trusted_proxies` から来たリクエストで読むヘッダー。既定は `X-Forwarded-*`、Fly.io に置くときは `"fly"`。
    pub proxy_headers: ProxyHeaders,
    /// このサーバーの外部公開URL (例: `https://bp.example.com`)。必須。Google OAuth のリダイレクトURIと、
    /// メール内リンクの組み立てに使う。
    #[serde(deserialize_with = "deserialize_public_url")]
    pub public_url: String,
    /// 「オープンβテスト中」の表示 (ヘッダーのバッジ・ログイン/サインアップ画面の一文) を出すか。
    /// 設定しなければ無効。本番・検証用は deploy/cloud の設定で出し分ける。
    pub beta_notice: bool,
    /// 利用者からの問い合わせ先の URL (例: 問い合わせフォーム)。必須。「運営者にお問い合わせください」の
    /// 文言にリンクを添える。
    #[serde(deserialize_with = "deserialize_contact_url")]
    pub contact_url: String,
    /// アプリを紹介するページの URL。設定しなければ、ログイン画面に紹介へのリンクを出さない
    /// (紹介ページを持たずに動かす人もいるため、必須にしない)。
    #[serde(deserialize_with = "deserialize_intro_url")]
    pub intro_url: String,
}

/// `contact_url` が、画面からリンクできる http(s) の URL であることを検証する (空は、書かれていないとして
/// 読み込みの後で弾く。→ `Config::check_required`)。
fn deserialize_contact_url<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: serde::Deserializer<'de>,
{
    parse_link_url("contact_url", String::deserialize(deserializer)?)
        .map_err(serde::de::Error::custom)
}

/// `intro_url` が、画面からリンクできる http(s) の URL であることを検証する (空は、リンクを出さない)。
fn deserialize_intro_url<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: serde::Deserializer<'de>,
{
    parse_link_url("intro_url", String::deserialize(deserializer)?)
        .map_err(serde::de::Error::custom)
}

/// 画面からリンクする URL を、http(s) に限って正規化する。空はそのまま返す。
fn parse_link_url(name: &str, value: String) -> Result<String, String> {
    if value.is_empty() {
        return Ok(value);
    }
    let url = reqwest::Url::parse(&value)
        .map_err(|err| format!("{name} を URL として解釈できません ({value}): {err}"))?;
    if !matches!(url.scheme(), "http" | "https") || url.host_str().is_none() {
        return Err(format!(
            "{name} は http または https のURLにしてください ({value})"
        ));
    }
    Ok(url.to_string())
}

/// `public_url` が、パスを後ろに付けてリンクにできる形であることを検証する (空は、書かれていないとして
/// 読み込みの後で弾く。→ `Config::check_required`)。
///
/// ADR: `http` を localhost に限らない。http のまま運用してもメール内リンクは成り立ち、
/// Google ログインで http が使えないことは Google Cloud Console の登録時に分かるため。
fn deserialize_public_url<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = String::deserialize(deserializer)?;
    if value.is_empty() {
        return Ok(value);
    }
    // 正規化した形 (スキーム・ホストは小文字) で持つ。`HTTP://` のような書き方でも、使う側が
    // 文字列でスキームを判定できるようにするため。
    let url = validate_public_url(&value).map_err(serde::de::Error::custom)?;
    Ok(url.to_string())
}

fn validate_public_url(value: &str) -> Result<reqwest::Url, String> {
    let url = reqwest::Url::parse(value)
        .map_err(|err| format!("public_url を URL として解釈できません ({value}): {err}"))?;
    if !matches!(url.scheme(), "http" | "https") {
        return Err(format!(
            "public_url は http または https で始めてください ({value})"
        ));
    }
    if url.host_str().is_none() {
        return Err(format!("public_url にホスト名がありません ({value})"));
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err(format!(
            "public_url にユーザー情報は含められません ({value})"
        ));
    }
    // アプリはルートで配信する前提で、パスを後ろに付けてリンクを作るため。
    if url.path() != "/" || url.query().is_some() || url.fragment().is_some() {
        return Err(format!(
            "public_url にはパス・クエリ・フラグメントを含められません ({value})"
        ));
    }
    Ok(url)
}

fn deserialize_optional_price<'de, D>(deserializer: D) -> Result<Option<f64>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    check_price(f64::deserialize(deserializer)?)
        .map(Some)
        .map_err(serde::de::Error::custom)
}

/// 単価は有限で 0 以上でなければならない (負値や NaN は累計を減らす・数え損なう原因になる)。
fn check_price(value: f64) -> Result<f64, String> {
    if !value.is_finite() || value < 0.0 {
        return Err(format!("単価は 0 以上の有限な数値にしてください ({value})"));
    }
    Ok(value)
}

fn deserialize_optional_topup_grant<'de, D>(deserializer: D) -> Result<Option<i64>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    check_topup_grant(i64::deserialize(deserializer)?)
        .map(Some)
        .map_err(serde::de::Error::custom)
}

/// 買い足しの量は正の値でなければならない (0 以下では買っても読み取れない)。
/// 1/1000円に直して持つため、直したときに溢れる値も弾く。
fn check_topup_grant(value: i64) -> Result<i64, String> {
    if value <= 0 || value.checked_mul(1000).is_none() {
        return Err(format!("買い足しの量は 1 以上の円にしてください ({value})"));
    }
    Ok(value)
}

/// `port` が 0 でないことを検証する。0 だと起動のたびに listen するポートが変わり、
/// ユーザーがアクセス先を知る手段が無くなる (実際に bind されるポートは
/// `TcpListener::bind` 実行時まで決まらないため、ログにも正しい値を出せない)。
fn deserialize_nonzero_port<'de, D>(deserializer: D) -> Result<u16, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = u16::deserialize(deserializer)?;
    if value == 0 {
        return Err(serde::de::Error::custom(
            "port は 0 にできません (OS任せの空きポートでは接続先が定まりません)",
        ));
    }
    Ok(value)
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            // 理由は `config.example.toml` の bind コメント参照
            bind: IpAddr::V4(Ipv4Addr::UNSPECIFIED),
            // デフォルトポート変更時は `config.example.toml` のサンプル値も変更する
            port: 3000,
            trusted_proxies: Vec::new(),
            proxy_headers: ProxyHeaders::default(),
            public_url: String::new(),
            beta_notice: false,
            contact_url: String::new(),
            intro_url: String::new(),
        }
    }
}

impl ServerConfig {
    pub fn socket_addr(&self) -> SocketAddr {
        SocketAddr::new(self.bind, self.port)
    }

    /// 末尾のスラッシュを除いた外部公開URL。パスを後ろに付けて使う。
    pub fn public_url(&self) -> &str {
        self.public_url.trim_end_matches('/')
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LogOutput {
    /// 標準出力。systemd / Docker 等がログを回収する運用向け。
    #[default]
    Stdout,
    /// `AppDirs::log_dir()` 配下に日次ローテーションで出力する。
    File,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct LogConfig {
    /// `tracing_subscriber::EnvFilter` の書式。環境変数 `RUST_LOG` があればそちらを優先する。
    pub filter: String,
    pub output: LogOutput,
}

impl Default for LogConfig {
    fn default() -> Self {
        Self {
            filter: "bp_carnet=debug,tower_http=debug".to_string(),
            output: LogOutput::default(),
        }
    }
}

// secret はログに出したくないため Debug を手書きし、値を伏せる。
#[derive(Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct SessionConfig {
    /// セッション Cookie の署名鍵 (base64 エンコードされた 64 バイト)。
    /// 空なら `AppDirs::session_key_path()` に自動生成・永続化する。
    pub secret: String,
    /// セッション Cookie に Secure 属性を付与するか。HTTPS 終端がある場合に true にする。
    pub secure_cookie: bool,
    /// 最終アクセスからこの日数が経過したセッションを失効させる。
    #[serde(deserialize_with = "deserialize_positive_expiry_days")]
    pub expiry_days: i64,
}

/// `expiry_days` が 1 以上であることを検証する。0 や負数だとセッションが作成前・作成直後に
/// 失効した壊れた状態になり得るため、設定ファイル読み込み時点で弾く。
fn deserialize_positive_expiry_days<'de, D>(deserializer: D) -> Result<i64, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = i64::deserialize(deserializer)?;
    if value < 1 {
        return Err(serde::de::Error::custom(format!(
            "expiry_days は 1 以上である必要があります ({value})"
        )));
    }
    Ok(value)
}

impl Default for SessionConfig {
    fn default() -> Self {
        Self {
            secret: String::new(),
            // 開発・テストは http で動かすため無効。HTTPS を終端する本番では設定で有効にする。
            // Secure 属性を付けたままだと非 HTTPS では Cookie が保存されずログインループになる。
            secure_cookie: false,
            // 高齢者向けアプリのため頻繁な再ログインを避け、長めに設定する。
            expiry_days: 30,
        }
    }
}

impl std::fmt::Debug for SessionConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SessionConfig")
            .field("secret", &"<redacted>")
            .field("secure_cookie", &self.secure_cookie)
            .field("expiry_days", &self.expiry_days)
            .finish()
    }
}

/// SMTP 接続の暗号化方式。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SmtpTls {
    /// 平文で接続してから STARTTLS で暗号化する (多くのサービスの 587 番ポート)。
    /// 暗号化できないサーバーには送らない。
    #[default]
    Starttls,
    /// 最初から TLS で接続する (465 番ポート)。
    Tls,
    /// 暗号化しない。同じマシンや同じネットワーク内の中継サーバーに渡す場合だけに使う。
    None,
}

/// メール送信 (SMTP) の接続情報 (docs/authentication.md)。パスワードは機微情報のため環境変数
/// `SMTP_PASSWORD` で渡し、ここには含めない。
#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct MailConfig {
    /// SMTP サーバーのホスト名。必須 (→ `Config::check_required`)。
    pub host: String,
    #[serde(deserialize_with = "deserialize_nonzero_port")]
    pub port: u16,
    /// 認証に使うユーザー名。空なら認証しない。
    pub username: String,
    /// 送信元 (例: `BP Carnet <noreply@example.com>`)。
    pub from: String,
    pub tls: SmtpTls,
}

impl Default for MailConfig {
    fn default() -> Self {
        Self {
            host: String::new(),
            // 既定の暗号化方式 (STARTTLS) で使う submission ポート。
            port: 587,
            username: String::new(),
            from: String::new(),
            tls: SmtpTls::Starttls,
        }
    }
}

/// パスワードに要求する文字の種類。設定ファイルの値であると同時に、入力欄に条件を示すため
/// frontend へ返す値でもある (`GET /auth/password-policy`)。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, serde::Serialize, utoipa::ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum CharClass {
    Digit,
    Lowercase,
    Uppercase,
    /// ASCII の英数字以外。記号のほか、日本語のようなマルチバイト文字も含む。
    Symbol,
}

impl CharClass {
    /// ADR: いずれも ASCII で判定する。Unicode の `is_lowercase` 等で判定すると、
    /// ひらがなやキリル文字が「英小文字」に該当してしまい、種類名と合わなくなる。
    pub fn contained_in(self, password: &str) -> bool {
        password.chars().any(|c| match self {
            Self::Digit => c.is_ascii_digit(),
            Self::Lowercase => c.is_ascii_lowercase(),
            Self::Uppercase => c.is_ascii_uppercase(),
            Self::Symbol => !c.is_ascii_alphanumeric(),
        })
    }

    /// 設定を直したい利用者向けのラベル (エラーメッセージに使う)。
    pub fn label(self) -> &'static str {
        match self {
            Self::Digit => "数字",
            Self::Lowercase => "英小文字",
            Self::Uppercase => "英大文字",
            Self::Symbol => "記号",
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(try_from = "RawPasswordConfig")]
pub struct PasswordConfig {
    /// 最小の文字数 (Unicode のコードポイント数で数える)。
    pub min_length: usize,
    /// 少なくとも1文字ずつ含めることを求める文字種。空なら文字種は問わない。
    pub required_classes: Vec<CharClass>,
}

impl Default for PasswordConfig {
    fn default() -> Self {
        Self {
            min_length: 8,
            required_classes: Vec::new(),
        }
    }
}

/// `PasswordConfig` の検証前の形。フィールドをまたぐ条件 (文字種の数と最小長の関係) を
/// 見るため、`try_from` を挟んで設定ファイルの読み込み時点で弾く。
#[derive(Deserialize)]
#[serde(default, deny_unknown_fields)]
struct RawPasswordConfig {
    min_length: usize,
    required_classes: Vec<CharClass>,
}

impl Default for RawPasswordConfig {
    fn default() -> Self {
        let PasswordConfig {
            min_length,
            required_classes,
        } = PasswordConfig::default();
        Self {
            min_length,
            required_classes,
        }
    }
}

impl TryFrom<RawPasswordConfig> for PasswordConfig {
    type Error = String;

    fn try_from(raw: RawPasswordConfig) -> Result<Self, Self::Error> {
        if raw.min_length < 1 {
            return Err("min_length は 1 以上である必要があります".to_string());
        }
        for (index, class) in raw.required_classes.iter().enumerate() {
            if raw.required_classes[..index].contains(class) {
                return Err(format!(
                    "required_classes に同じ文字種が重複しています ({})",
                    class.label()
                ));
            }
        }
        // 満たしようのない設定で、パスワードを何度入力し直しても登録できない状態を防ぐ。
        if raw.required_classes.len() > raw.min_length {
            return Err(format!(
                "min_length ({}) が required_classes の数 ({}) を下回っています",
                raw.min_length,
                raw.required_classes.len()
            ));
        }
        Ok(Self {
            min_length: raw.min_length,
            required_classes: raw.required_classes,
        })
    }
}

/// OCR の設定。無料枠・買い足しの量・単価は、環境変数 (`OCR_*`) があればそちらを使う
/// (→ [`OcrConfig::apply_env`])。コードに既定値は持たず、機能を有効にするときに要る
/// (→ [`OcrConfig::costs`])。
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct OcrConfig {
    /// 1ユーザーが OCR に使える累計の金額 (円。無料枠)。負値なら無制限。ユーザーごとの上限
    /// (`users.ocr_budget_yen`) が設定されていればそちらが優先される。
    pub free_budget_yen: Option<i64>,
    /// 買い足し1回で付ける量 (円)。
    #[serde(deserialize_with = "deserialize_optional_topup_grant")]
    pub topup_grant_yen: Option<i64>,
    /// 使った額を API の `usage` から計算するための単価。
    pub pricing: OcrPricingConfig,
    /// Gemini とのやりとりを残すディレクトリ (docs/ocr.md)。相対パスはデータディレクトリ基準。
    pub dump_dir: Option<PathBuf>,
    /// 開発用。`dump_dir` に同じ送信内容で成功したやりとりが残っていれば、Gemini を呼ばずにその応答を返す。
    pub reuse_dumps: bool,
}

/// `[ocr.pricing]`。3つそろったときに [`OcrPricing`] になる。
#[derive(Debug, Clone, Copy, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct OcrPricingConfig {
    /// 入力 100 万トークンあたりのドル。
    #[serde(deserialize_with = "deserialize_optional_price")]
    pub input_usd_per_million_tokens: Option<f64>,
    /// 出力 (思考を含む) 100 万トークンあたりのドル。
    #[serde(deserialize_with = "deserialize_optional_price")]
    pub output_usd_per_million_tokens: Option<f64>,
    /// 1ドルの円換算。
    #[serde(deserialize_with = "deserialize_optional_price")]
    pub jpy_per_usd: Option<f64>,
}

/// Gemini の単価 (DEVELOPMENT.md「写真の読み取りの無料枠・単価を変える」)。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct OcrPricing {
    /// 入力 100 万トークンあたりのドル。
    pub input_usd_per_million_tokens: f64,
    /// 出力 (思考を含む) 100 万トークンあたりのドル。
    pub output_usd_per_million_tokens: f64,
    /// 1ドルの円換算。
    pub jpy_per_usd: f64,
}

impl OcrPricing {
    /// 1回の読み取りの費用 (1/1000円)。小数は切り上げる (実際より少なく数えないため)。
    pub fn cost_milli_yen(&self, input_tokens: i64, output_tokens: i64) -> i64 {
        let usd = (input_tokens.max(0) as f64 * self.input_usd_per_million_tokens
            + output_tokens.max(0) as f64 * self.output_usd_per_million_tokens)
            / 1_000_000.0;
        // f64 → i64 の `as` は範囲外で飽和する。
        (usd * self.jpy_per_usd * 1000.0).ceil() as i64
    }
}

/// 起動時にそろっていることを確かめた、OCR の無料枠・単価と買い足しの量 (→ [`OcrConfig::costs`])。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct OcrCosts {
    /// 既定の無料枠 (円)。負値は無制限。
    pub free_budget_yen: i64,
    pub pricing: OcrPricing,
    /// 買い足し1回で付ける量 (1/1000円)。
    pub topup_grant_milli_yen: i64,
}

/// 無料枠・買い足しの量・単価を与える環境変数。本番は Fly のシークレットで与える (DEVELOPMENT.md)。
pub const OCR_FREE_BUDGET_YEN_ENV: &str = "OCR_FREE_BUDGET_YEN";
pub const OCR_TOPUP_GRANT_YEN_ENV: &str = "OCR_TOPUP_GRANT_YEN";
pub const OCR_INPUT_USD_PER_MILLION_TOKENS_ENV: &str = "OCR_INPUT_USD_PER_MILLION_TOKENS";
pub const OCR_OUTPUT_USD_PER_MILLION_TOKENS_ENV: &str = "OCR_OUTPUT_USD_PER_MILLION_TOKENS";
pub const OCR_JPY_PER_USD_ENV: &str = "OCR_JPY_PER_USD";

impl OcrConfig {
    /// 環境変数で、config.toml の無料枠・買い足しの量・単価を上書きする。`env` は値を引く関数
    /// (未設定と空は `None`。→ [`non_empty_env`])。
    pub fn apply_env(&mut self, env: impl Fn(&str) -> Option<String>) -> Result<(), Error> {
        fn parse<T: std::str::FromStr>(
            name: &'static str,
            value: &str,
            check: impl Fn(T) -> Result<T, String>,
        ) -> Result<T, Error>
        where
            T::Err: std::fmt::Display,
        {
            let parsed = value.parse::<T>().map_err(|err| Error::Env {
                name,
                message: format!("{err} ({value})"),
            })?;
            check(parsed).map_err(|message| Error::Env { name, message })
        }

        if let Some(value) = env(OCR_FREE_BUDGET_YEN_ENV) {
            self.free_budget_yen = Some(parse(OCR_FREE_BUDGET_YEN_ENV, &value, Ok)?);
        }
        if let Some(value) = env(OCR_TOPUP_GRANT_YEN_ENV) {
            self.topup_grant_yen = Some(parse(OCR_TOPUP_GRANT_YEN_ENV, &value, check_topup_grant)?);
        }
        let prices = [
            (
                OCR_INPUT_USD_PER_MILLION_TOKENS_ENV,
                &mut self.pricing.input_usd_per_million_tokens,
            ),
            (
                OCR_OUTPUT_USD_PER_MILLION_TOKENS_ENV,
                &mut self.pricing.output_usd_per_million_tokens,
            ),
            (OCR_JPY_PER_USD_ENV, &mut self.pricing.jpy_per_usd),
        ];
        for (name, slot) in prices {
            if let Some(value) = env(name) {
                *slot = Some(parse(name, &value, check_price)?);
            }
        }
        Ok(())
    }

    /// 有効な機能に要る値がそろっているかを確かめて返す。欠けていれば、何が無いかの文言を返す。
    /// 写真の読み取り (`ocr_enabled`) には無料枠と単価、買い足し (`topup_enabled`) には買い足しの量が要る。
    /// 無効な機能の値は使わないので、欠けていれば何も数えない値 (無制限・0) にする。
    pub fn costs(&self, ocr_enabled: bool, topup_enabled: bool) -> Result<OcrCosts, String> {
        let missing = |feature: &str, env: &str, key: &str| {
            format!("{feature} が有効ですが、{env} (または config.toml の {key}) がありません")
        };
        let ocr_feature = "写真の読み取り (GEMINI_API_KEY)";
        let free_budget_yen = match self.free_budget_yen {
            Some(value) => value,
            None if ocr_enabled => {
                return Err(missing(
                    ocr_feature,
                    OCR_FREE_BUDGET_YEN_ENV,
                    "[ocr] free_budget_yen",
                ));
            }
            None => -1,
        };
        let pricing = &self.pricing;
        let pricing = match (
            pricing.input_usd_per_million_tokens,
            pricing.output_usd_per_million_tokens,
            pricing.jpy_per_usd,
        ) {
            (Some(input), Some(output), Some(jpy)) => OcrPricing {
                input_usd_per_million_tokens: input,
                output_usd_per_million_tokens: output,
                jpy_per_usd: jpy,
            },
            (input, output, _) if ocr_enabled => {
                let (env, key) = if input.is_none() {
                    (
                        OCR_INPUT_USD_PER_MILLION_TOKENS_ENV,
                        "[ocr.pricing] input_usd_per_million_tokens",
                    )
                } else if output.is_none() {
                    (
                        OCR_OUTPUT_USD_PER_MILLION_TOKENS_ENV,
                        "[ocr.pricing] output_usd_per_million_tokens",
                    )
                } else {
                    (OCR_JPY_PER_USD_ENV, "[ocr.pricing] jpy_per_usd")
                };
                return Err(missing(ocr_feature, env, key));
            }
            _ => OcrPricing {
                input_usd_per_million_tokens: 0.0,
                output_usd_per_million_tokens: 0.0,
                jpy_per_usd: 0.0,
            },
        };
        let topup_grant_milli_yen = match self.topup_grant_yen {
            // 読み込みのときに、1/1000円に直しても溢れないことを確かめている。
            Some(value) => value * 1000,
            None if topup_enabled => {
                return Err(missing(
                    "読み取りの買い足し (Stripe かアプリ内課金)",
                    OCR_TOPUP_GRANT_YEN_ENV,
                    "[ocr] topup_grant_yen",
                ));
            }
            None => 0,
        };
        Ok(OcrCosts {
            free_budget_yen,
            pricing,
            topup_grant_milli_yen,
        })
    }

    /// `dump_dir` を `data_dir` 基準で解決する。空文字は未設定として扱う。
    pub fn dump_dir_in(&self, data_dir: &Path) -> Option<PathBuf> {
        self.dump_dir
            .as_ref()
            .filter(|dir| !dir.as_os_str().is_empty())
            .map(|dir| data_dir.join(dir))
    }
}

/// 放置アカウントの自動退会 (docs/authentication.md)。
#[derive(Debug, Clone, Copy, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct InactivityConfig {
    /// 最終アクセスからこの日数がたったアカウントを退会させる (削除予約を入れる)。0 なら無効。
    /// 設定しなければ無効。開発・テストでは 0 のままにし、本番は deploy/cloud の設定で有効にする。
    pub delete_after_days: u32,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct MobileAppConfig {
    /// API を呼べるモバイルアプリの一番古い版 (`crate::app_version`)。これより古い版には更新を促す。
    /// 無ければ版を見ない。開発・テストでは未設定のままにし、本番は deploy/cloud の設定で有効にする。
    pub min_version: Option<AppVersion>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    pub server: ServerConfig,
    pub log: LogConfig,
    pub session: SessionConfig,
    pub mail: MailConfig,
    pub ocr: OcrConfig,
    pub inactivity: InactivityConfig,
    pub password: PasswordConfig,
    pub payments: PaymentsConfig,
    pub mobile_app: MobileAppConfig,
}

/// OCR 枠買い足し (→ docs/payments.md) のうち、秘密でない設定。
/// `secret_key`・`webhook_secret`・Price ID は機微情報のため config.toml には置かず、
/// 環境変数から読む (`crate::payments::stripe`、`GoogleLoginClient` と同じ方針)。
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct PaymentsConfig {
    pub stripe: StripeConfig,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct StripeConfig {
    /// 本番で true にする。環境変数の秘密がそろっていても、これが false なら
    /// 買い足し導線・Webhook を無効のままにする。
    pub enabled: bool,
    /// Checkout Session 完了後に戻る URL。`{CHECKOUT_SESSION_ID}` を含めて、
    /// 結果確認 (`GET /payments/ocr-topup/result`) に session_id を渡せるようにする。
    pub checkout_success_url: String,
    pub checkout_cancel_url: String,
}

impl Config {
    /// `dirs.config_path()` を読み込む。必須の設定 (→ [`Self::check_required`]) が無ければエラーにする。
    /// 無料枠・買い足しの量・単価は、環境変数があればそちらを使う (→ [`OcrConfig::apply_env`])。
    pub fn load(dirs: &AppDirs) -> Result<Self, Error> {
        Self::load_from(&dirs.config_path(), non_empty_env)
    }

    fn load_from(path: &Path, env: impl Fn(&str) -> Option<String>) -> Result<Self, Error> {
        let text = std::fs::read_to_string(path).map_err(|source| Error::Read {
            path: path.to_path_buf(),
            source,
        })?;
        let mut config: Self = toml::from_str(&text).map_err(|source| Error::Parse {
            path: path.to_path_buf(),
            source,
        })?;
        config.ocr.apply_env(env)?;
        config
            .check_required()
            .map_err(|name| Error::Missing(name, path.to_path_buf()))?;
        Ok(config)
    }

    /// 本番で必ず入れる設定がそろっているか。無ければ、欠けている設定の名前を返す。
    ///
    /// ADR: 未設定のときに機能ごと切る作りにしない。公開しているのは本番の1か所だけで、
    /// 切れた画面は開発で確かめられなくなるだけのため。開発・テストでは、本番と同じ値か
    /// ダミーの値を入れる (メールは手元の SMTP サーバーで受ける。→ `just dev-mail`)。
    /// 外部サービスの鍵 (Gemini・Stripe など) は、ダミーでは呼べないので対象外 (無ければ機能ごと切る)。
    pub fn check_required(&self) -> Result<(), &'static str> {
        let required = [
            ("[server] public_url", &self.server.public_url),
            ("[server] contact_url", &self.server.contact_url),
            ("[mail] host", &self.mail.host),
            ("[mail] from", &self.mail.from),
        ];
        match required.into_iter().find(|(_, value)| value.is_empty()) {
            Some((name, _)) => Err(name),
            None => Ok(()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ocr_pricing_computes_the_cost_in_milli_yen_and_rounds_up() {
        let pricing = OcrPricing {
            input_usd_per_million_tokens: 1.0,
            output_usd_per_million_tokens: 4.0,
            jpy_per_usd: 100.0,
        };
        // 入力 1,000・出力 1,000 トークン: (1 + 4) / 1000 ドル × 100円 = 0.5円。
        assert_eq!(pricing.cost_milli_yen(1_000, 1_000), 500);
        // 0.0004 円 (1トークン) は 1/1000 円に切り上げる。
        assert_eq!(pricing.cost_milli_yen(0, 1), 1);
        assert_eq!(pricing.cost_milli_yen(0, 0), 0);
    }

    #[test]
    fn ocr_pricing_rejects_negative_or_non_finite_values() {
        for bad in ["-1.0", "nan", "inf"] {
            let toml = format!("[ocr.pricing]\njpy_per_usd = {bad}\n");
            assert!(toml::from_str::<Config>(&toml).is_err(), "{bad}");
        }
    }

    #[test]
    fn ocr_topup_grant_must_be_positive() {
        for bad in ["0", "-1", "9223372036854775807"] {
            let toml = format!("[ocr]\ntopup_grant_yen = {bad}\n");
            assert!(toml::from_str::<Config>(&toml).is_err(), "{bad}");
        }
    }

    fn ocr_config(text: &str) -> OcrConfig {
        toml::from_str::<Config>(text)
            .expect("ocr config should parse")
            .ocr
    }

    const COMPLETE_OCR: &str = "[ocr]\nfree_budget_yen = 30\ntopup_grant_yen = 40\n[ocr.pricing]\ninput_usd_per_million_tokens = 1.0\noutput_usd_per_million_tokens = 4.0\njpy_per_usd = 100.0\n";

    #[test]
    fn ocr_costs_come_from_the_config_file() {
        let costs = ocr_config(COMPLETE_OCR)
            .costs(true, true)
            .expect("all values are set");
        assert_eq!(
            costs,
            OcrCosts {
                free_budget_yen: 30,
                pricing: OcrPricing {
                    input_usd_per_million_tokens: 1.0,
                    output_usd_per_million_tokens: 4.0,
                    jpy_per_usd: 100.0,
                },
                topup_grant_milli_yen: 40_000,
            }
        );
    }

    #[test]
    fn ocr_costs_from_the_environment_take_precedence_over_the_config_file() {
        let mut config = ocr_config(COMPLETE_OCR);
        config
            .apply_env(|name| {
                match name {
                    OCR_FREE_BUDGET_YEN_ENV => Some("-1"),
                    OCR_TOPUP_GRANT_YEN_ENV => Some("50"),
                    OCR_INPUT_USD_PER_MILLION_TOKENS_ENV => Some("2"),
                    OCR_OUTPUT_USD_PER_MILLION_TOKENS_ENV => Some("8.5"),
                    OCR_JPY_PER_USD_ENV => Some("110"),
                    _ => None,
                }
                .map(str::to_owned)
            })
            .expect("valid environment values");
        let costs = config.costs(true, true).expect("all values are set");
        assert_eq!(costs.free_budget_yen, -1);
        assert_eq!(costs.topup_grant_milli_yen, 50_000);
        assert_eq!(
            costs.pricing,
            OcrPricing {
                input_usd_per_million_tokens: 2.0,
                output_usd_per_million_tokens: 8.5,
                jpy_per_usd: 110.0,
            }
        );
    }

    #[test]
    fn invalid_ocr_cost_environment_values_are_rejected() {
        for (name, value) in [
            (OCR_FREE_BUDGET_YEN_ENV, "30.5"),
            (OCR_TOPUP_GRANT_YEN_ENV, "0"),
            (OCR_TOPUP_GRANT_YEN_ENV, "abc"),
            (OCR_INPUT_USD_PER_MILLION_TOKENS_ENV, "-1"),
            (OCR_JPY_PER_USD_ENV, "nan"),
        ] {
            let mut config = OcrConfig::default();
            let result = config.apply_env(|asked| (asked == name).then(|| value.to_owned()));
            assert!(
                matches!(result, Err(Error::Env { name: got, .. }) if got == name),
                "{name}={value}: {result:?}"
            );
        }
    }

    #[test]
    fn ocr_costs_are_required_only_for_enabled_features() {
        let empty = OcrConfig::default();
        let costs = empty
            .costs(false, false)
            .expect("disabled features need no values");
        assert_eq!(costs.free_budget_yen, -1);
        assert_eq!(costs.topup_grant_milli_yen, 0);

        let without_budget = ocr_config(&COMPLETE_OCR.replace("free_budget_yen = 30\n", ""));
        let err = without_budget
            .costs(true, false)
            .expect_err("OCR needs the free budget");
        assert!(err.contains(OCR_FREE_BUDGET_YEN_ENV), "{err}");

        let without_rate = ocr_config(&COMPLETE_OCR.replace("jpy_per_usd = 100.0\n", ""));
        let err = without_rate
            .costs(true, false)
            .expect_err("OCR needs every price");
        assert!(err.contains(OCR_JPY_PER_USD_ENV), "{err}");

        let without_grant = ocr_config(&COMPLETE_OCR.replace("topup_grant_yen = 40\n", ""));
        assert!(without_grant.costs(true, false).is_ok());
        let err = without_grant
            .costs(true, true)
            .expect_err("purchases need the grant");
        assert!(err.contains(OCR_TOPUP_GRANT_YEN_ENV), "{err}");
    }

    /// 無料枠・買い足しの量・単価は、デプロイの設定ファイルでなく環境変数で与える。
    #[test]
    fn cloud_config_files_parse_without_the_ocr_costs() {
        for text in [
            include_str!("../deploy/cloud/config.production.toml"),
            include_str!("../deploy/cloud/config.staging.toml"),
        ] {
            let config: Config = toml::from_str(text).expect("cloud config should parse");
            assert_eq!(config.check_required(), Ok(()));
            let ocr = &config.ocr;
            assert_eq!(ocr.free_budget_yen, None);
            assert_eq!(ocr.topup_grant_yen, None);
            assert_eq!(ocr.pricing.input_usd_per_million_tokens, None);
            assert_eq!(ocr.pricing.output_usd_per_million_tokens, None);
            assert_eq!(ocr.pricing.jpy_per_usd, None);
        }
    }

    #[test]
    fn example_file_parses_with_the_required_settings() {
        let text = include_str!("../config.example.toml");
        let config: Config = toml::from_str(text).expect("config.example.toml should parse");
        assert_eq!(config.check_required(), Ok(()));
        assert_eq!(config.server.bind, IpAddr::V4(Ipv4Addr::UNSPECIFIED));
        assert_eq!(config.server.port, 3000);
        assert!(config.server.trusted_proxies.is_empty());
        assert_eq!(config.server.proxy_headers, ProxyHeaders::XForwarded);
        assert_eq!(config.password.min_length, 8);
        assert!(config.password.required_classes.is_empty());
        assert!(!config.server.beta_notice);
        assert_eq!(config.log.filter, LogConfig::default().filter);
        assert_eq!(config.log.output, LogOutput::default());
        assert_eq!(config.session.secret, "");
        assert!(!config.session.secure_cookie);
        assert_eq!(config.session.expiry_days, 30);
    }

    #[test]
    fn missing_required_settings_are_named() {
        let complete: Config =
            toml::from_str(include_str!("../config.example.toml")).expect("例は読めるはず");
        let mut config = complete.clone();
        config.server.contact_url.clear();
        assert_eq!(config.check_required(), Err("[server] contact_url"));
        let mut config = complete;
        config.mail.host.clear();
        assert_eq!(config.check_required(), Err("[mail] host"));
    }

    #[test]
    fn config_without_required_settings_is_not_loaded() {
        let tmp = crate::test_support::TempDir::new("config-missing-required");
        std::fs::create_dir_all(tmp.path()).expect("一時ディレクトリを作れるはず");
        let path = tmp.path().join("config.toml");
        std::fs::write(&path, "[server]\nport = 3000\n").expect("書けるはず");
        assert!(matches!(
            Config::load_from(&path, |_| None),
            Err(Error::Missing("[server] public_url", _))
        ));
    }

    #[test]
    fn public_url_accepts_http_and_https_origins() {
        for value in ["https://bp.example.com", "http://raspberrypi.local:3000/"] {
            let text = format!("[server]\npublic_url = \"{value}\"\n");
            toml::from_str::<Config>(&text).unwrap_or_else(|err| panic!("{value}: {err}"));
        }
    }

    #[test]
    fn public_url_rejects_values_that_cannot_prefix_a_link() {
        for value in [
            "bp.example.com",
            "ftp://bp.example.com",
            "https://user:pass@bp.example.com",
            "https://bp.example.com/app",
            "https://bp.example.com/?a=1",
            "https://bp.example.com/#x",
        ] {
            let text = format!("[server]\npublic_url = \"{value}\"\n");
            assert!(toml::from_str::<Config>(&text).is_err(), "{value}");
        }
    }

    #[test]
    fn public_url_drops_trailing_slash() {
        let config: Config = toml::from_str("[server]\npublic_url = \"https://bp.example.com/\"\n")
            .expect("public_url should parse");
        assert_eq!(config.server.public_url(), "https://bp.example.com");
    }

    /// 大文字で書いても正規化して持つ (http のときの起動時の警告が、文字列の比較で判定するため)。
    #[test]
    fn public_url_is_normalized_to_lowercase_scheme_and_host() {
        let config: Config = toml::from_str("[server]\npublic_url = \"HTTP://BP.Example.com\"\n")
            .expect("public_url should parse");
        assert_eq!(config.server.public_url(), "http://bp.example.com");
    }

    #[test]
    fn intro_url_is_optional_and_accepts_only_http_and_https_links() {
        let config: Config = toml::from_str("[server]\n").expect("intro_url may be omitted");
        assert_eq!(config.server.intro_url, "");
        let config: Config =
            toml::from_str("[server]\nintro_url = \"https://amiiby.com/bp-carnet/\"\n")
                .expect("intro_url should parse");
        assert_eq!(config.server.intro_url, "https://amiiby.com/bp-carnet/");
        let text = "[server]\nintro_url = \"javascript:alert(1)\"\n";
        assert!(toml::from_str::<Config>(text).is_err());
    }

    #[test]
    fn contact_url_accepts_only_http_and_https_links() {
        let config: Config =
            toml::from_str("[server]\ncontact_url = \"https://amiiby.com/about/\"\n")
                .expect("contact_url should parse");
        assert_eq!(config.server.contact_url, "https://amiiby.com/about/");
        for value in [
            "amiiby.com/about/",
            "mailto:a@example.com",
            "javascript:alert(1)",
        ] {
            let text = format!("[server]\ncontact_url = \"{value}\"\n");
            assert!(toml::from_str::<Config>(&text).is_err(), "{value}");
        }
    }

    /// `[oauth]` 節は存在しないため弾く。黙って無視すると Google ログインが理由不明のまま無効になる。
    #[test]
    fn unknown_oauth_section_is_rejected() {
        let result: Result<Config, _> = toml::from_str("[oauth]\nbase_url = \"https://x\"\n");
        assert!(result.is_err());
    }

    #[test]
    fn empty_input_uses_defaults() {
        let config: Config = toml::from_str("").expect("empty input should use defaults");
        assert_eq!(config.server.socket_addr().port(), 3000);
        assert_eq!(config.log.output, LogOutput::Stdout);
    }

    #[test]
    fn ocr_dump_dir_is_off_unless_set() {
        let config: Config = toml::from_str("").expect("empty input should use defaults");
        assert_eq!(config.ocr.dump_dir, None);
        assert!(!config.ocr.reuse_dumps);

        let config: Config =
            toml::from_str("[ocr]\ndump_dir = \"ocr-dumps\"\n").expect("dump_dir should parse");
        assert_eq!(config.ocr.dump_dir, Some(PathBuf::from("ocr-dumps")));
    }

    #[test]
    fn ocr_dump_dir_resolves_against_the_data_dir() {
        let data_dir = Path::new("/data");
        let dump_dir_in = |value: &str| {
            let config: Config = toml::from_str(&format!("[ocr]\ndump_dir = \"{value}\"\n"))
                .expect("dump_dir should parse");
            config.ocr.dump_dir_in(data_dir)
        };
        assert_eq!(
            dump_dir_in("ocr-dumps"),
            Some(PathBuf::from("/data/ocr-dumps"))
        );
        assert_eq!(dump_dir_in("/var/dumps"), Some(PathBuf::from("/var/dumps")));
        assert_eq!(dump_dir_in(""), None);
    }

    #[test]
    fn partial_server_keeps_bind_default() {
        let config: Config =
            toml::from_str("[server]\nport = 1\n").expect("partial server section should parse");
        assert_eq!(config.server.bind, IpAddr::V4(Ipv4Addr::UNSPECIFIED));
        assert_eq!(config.server.port, 1);
    }

    #[test]
    fn zero_port_is_rejected() {
        let result: Result<Config, _> = toml::from_str("[server]\nport = 0\n");
        assert!(result.is_err());
    }

    /// `try_from` を挟んだことで、節を書いて中身を空にした場合も既定値の経路になる。
    #[test]
    fn empty_password_section_falls_back_to_defaults() {
        let config = toml::from_str::<Config>("[password]\n").expect("空の節は既定値になるべき");
        assert_eq!(config.password.min_length, 8);
        assert!(config.password.required_classes.is_empty());
    }

    #[test]
    fn password_min_length_of_zero_is_rejected() {
        let err = toml::from_str::<Config>("[password]\nmin_length = 0\n")
            .expect_err("空のパスワードを許す設定は弾くべき");
        assert!(err.to_string().contains("min_length"), "{err}");
    }

    #[test]
    fn duplicated_required_classes_are_rejected() {
        let err =
            toml::from_str::<Config>("[password]\nrequired_classes = [\"digit\", \"digit\"]\n")
                .expect_err("重複した文字種は弾くべき");
        assert!(err.to_string().contains("重複"), "{err}");
    }

    /// どう入力しても満たせない設定 (必要な文字種の数 > 最小長) を弾く。
    #[test]
    fn required_classes_longer_than_min_length_are_rejected() {
        let err = toml::from_str::<Config>(
            "[password]\nmin_length = 2\nrequired_classes = [\"digit\", \"lowercase\", \"uppercase\"]\n",
        )
        .expect_err("満たしようのない設定は弾くべき");
        assert!(err.to_string().contains("min_length"), "{err}");
    }

    #[test]
    fn unknown_field_is_rejected() {
        let result: Result<Config, _> = toml::from_str("[server]\nprot = 8080\n");
        assert!(result.is_err());
    }

    #[test]
    fn unknown_log_output_is_rejected() {
        let result: Result<Config, _> = toml::from_str("[log]\noutput = \"syslog\"\n");
        assert!(result.is_err());
    }

    #[test]
    fn zero_expiry_days_is_rejected() {
        let result: Result<Config, _> = toml::from_str("[session]\nexpiry_days = 0\n");
        assert!(result.is_err());
    }

    #[test]
    fn negative_expiry_days_is_rejected() {
        let result: Result<Config, _> = toml::from_str("[session]\nexpiry_days = -1\n");
        assert!(result.is_err());
    }

    #[test]
    fn positive_expiry_days_is_accepted() {
        let config: Config = toml::from_str("[session]\nexpiry_days = 1\n")
            .expect("expiry_days of 1 should be accepted");
        assert_eq!(config.session.expiry_days, 1);
    }

    #[test]
    fn missing_file_is_an_error() {
        let err = Config::load_from(Path::new("does/not/exist/config.toml"), |_| None)
            .expect_err("設定ファイルが無ければ起動しないはず");
        assert!(matches!(err, Error::Read { .. }), "{err:?}");
    }

    #[test]
    fn proxy_headers_accepts_fly() {
        let config: Config =
            toml::from_str("[server]\nproxy_headers = \"fly\"").expect("fly を読めるはず");
        assert_eq!(config.server.proxy_headers, ProxyHeaders::Fly);
        assert!(toml::from_str::<Config>("[server]\nproxy_headers = \"cloudflare\"").is_err());
    }

    #[test]
    fn trusted_proxies_accept_addresses_and_cidr_ranges() {
        let config: Config =
            toml::from_str("[server]\ntrusted_proxies = [\"127.0.0.1\", \"172.18.0.0/16\"]")
                .expect("アドレスと範囲を読めるはず");
        assert_eq!(
            config.server.trusted_proxies,
            vec![
                "127.0.0.1".parse::<TrustedProxy>().expect("固定値"),
                "172.18.0.0/16".parse::<TrustedProxy>().expect("固定値"),
            ]
        );

        let error = toml::from_str::<Config>("[server]\ntrusted_proxies = [\"172.18.0.3/16\"]")
            .expect_err("ホスト部にビットが立った範囲は弾くはず");
        assert!(error.to_string().contains("172.18.0.3/16"), "{error}");
    }

    #[test]
    fn parse_error_keeps_line_number_detail() {
        let tmp = crate::test_support::TempDir::new("config-parse-error");
        std::fs::create_dir_all(tmp.path()).expect("failed to create the temp dir");
        let path = tmp.path().join("config.toml");
        std::fs::write(&path, "[server]\nport = \"abc\"\n")
            .expect("failed to write the temp config file");

        let err =
            Config::load_from(&path, |_| None).expect_err("invalid port should fail to parse");
        match &err {
            Error::Parse { source, .. } => {
                assert!(source.to_string().contains("line 2"), "{source}");
            }
            other => panic!("expected Error::Parse, got {other:?}"),
        }
    }

    #[test]
    fn app_dirs_derive_paths_from_data_dir() {
        let dirs = AppDirs {
            config_dir: PathBuf::from("/cfg"),
            data_dir: PathBuf::from("/data"),
        };
        assert_eq!(dirs.config_path(), Path::new("/cfg/config.toml"));
        assert_eq!(dirs.db_path(), Path::new("/data/bp-carnet.db"));
        assert_eq!(dirs.session_key_path(), Path::new("/data/session.key"));
        assert_eq!(dirs.log_dir(), Path::new("/data/logs"));
        assert_eq!(dirs.lock_path(), Path::new("/data/bp-carnet.lock"));
    }

    #[test]
    fn db_path_falls_back_to_legacy_name_only_when_it_exists() {
        let tmp = crate::test_support::TempDir::new("config-legacy-db");
        let dir = tmp.path().to_path_buf();
        std::fs::create_dir_all(&dir).expect("テスト用ファイルの操作");
        let dirs = AppDirs {
            config_dir: dir.clone(),
            data_dir: dir.clone(),
        };
        assert_eq!(dirs.db_path(), dir.join("bp-carnet.db"));

        std::fs::write(dir.join("bp-tracker.db"), b"").expect("テスト用ファイルの操作");
        assert_eq!(dirs.db_path(), dir.join("bp-tracker.db"));

        std::fs::write(dir.join("bp-carnet.db"), b"").expect("テスト用ファイルの操作");
        assert_eq!(dirs.db_path(), dir.join("bp-carnet.db"));
    }

    #[test]
    fn session_config_debug_redacts_secret() {
        let config = SessionConfig {
            secret: "super-secret".to_string(),
            ..Default::default()
        };
        let debug = format!("{config:?}");
        assert!(!debug.contains("super-secret"), "{debug}");
        assert!(debug.contains("<redacted>"), "{debug}");
    }
}
