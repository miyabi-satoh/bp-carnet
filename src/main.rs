use std::path::PathBuf;
use std::sync::Arc;

use bp_carnet::apple_login::AppleLoginClient;
use bp_carnet::config::{AppDirs, Config};
use bp_carnet::google_login::GoogleLoginClient;
use bp_carnet::line_login::LineLoginClient;
use bp_carnet::mail::Mailer;
use bp_carnet::ocr::OcrService;
use bp_carnet::payments::app_store::AppStoreClient;
use bp_carnet::payments::stripe::StripeClient;
use bp_carnet::{cli, db, error_chain, error_chain_line, logging, session, single_instance};
use tokio::net::TcpListener;
use tower_sessions::cookie::Key;
use tower_sessions::session_store::ExpiredDeletion;
use tower_sessions_sqlx_store::SqliteStore;
use tracing_appender::non_blocking::WorkerGuard;

type BoxError = Box<dyn std::error::Error + Send + Sync>;

fn main() {
    // API キー等、config.toml に書かない設定値を環境変数として反映するため、
    // `AppDirs::resolve` (BP_CARNET_HOME を読む) より前に呼ぶ。.env が無いのは正常系
    // (`not_found`) だが、構文エラー等は放置すると OAuth/OCR が理由不明のまま無効化される
    // だけになるため、ログ初期化前でも exit_with_error で落とす。
    if let Err(err) = dotenvy::dotenv()
        && !err.not_found()
    {
        exit_with_error(&err);
    }

    if cli::handle_args() {
        return;
    }

    let dirs = AppDirs::resolve().unwrap_or_else(|err| exit_with_error(&err));
    let config = Config::load(&dirs).unwrap_or_else(|err| exit_with_error(&err));

    // `WorkerGuard` は非同期書き込みワーカーの生存期間を握っている。drop するとバッファ中の
    // ログが書き出されてから消えるため、終了するまで保持し、終了の直前に drop する
    // (`exit_with_log`)。drop しないまま `process::exit` すると、最後のログが残らない。
    let log_guard = logging::init(&config.log, &dirs.log_dir()).unwrap_or_else(|err| {
        eprintln!("ログ初期化に失敗しました: {err}");
        std::process::exit(1);
    });

    tracing::info!(
        version = env!("CARGO_PKG_VERSION"),
        bind = %config.server.socket_addr(),
        config_path = %dirs.config_path().display(),
        data_dir = %dirs.data_dir.display(),
        "起動します"
    );

    // 同じデータディレクトリに対して config.toml の port を変えて誤って複数回起動する、
    // just dev-backend を二重に実行してしまう、といったケースで DB・セッション鍵への
    // 同時アクセスを防ぐため、DB 接続より前に二重起動を検知する。
    let _instance_lock = match single_instance::acquire(&dirs.lock_path()) {
        Ok(Some(lock)) => lock,
        Ok(None) => exit_with_log(log_guard, "既に起動しています (別プロセスが起動中です)"),
        Err(err) => exit_with_log(
            log_guard,
            &format!("シングルインスタンスロックの取得に失敗しました: {err}"),
        ),
    };

    if !config.session.secure_cookie && !config.server.bind.is_loopback() {
        tracing::warn!(
            "session.secure_cookie が無効のため、セッション Cookie に Secure 属性を付与しません。HTTPS で公開する場合は true に設定してください。"
        );
    }

    let ocr_dump_dir = config.ocr.dump_dir_in(&dirs.data_dir);
    let ocr = OcrService::from_env()
        .with_dump_dir(ocr_dump_dir.clone())
        .with_reuse_dumps(config.ocr.reuse_dumps);
    if ocr.enabled() {
        tracing::info!("OCR (Gemini) 機能が有効です");
    } else {
        tracing::info!("GEMINI_API_KEY が未設定のため、OCR 機能は無効です");
    }
    if let Some(dir) = ocr_dump_dir.as_ref().filter(|_| ocr.enabled()) {
        tracing::warn!(
            dir = %dir.display(),
            "Gemini とのやりとり (送った写真を含む) を保存します。調べ終えたら [ocr] dump_dir を外してください"
        );
        if config.ocr.reuse_dumps {
            tracing::warn!(
                "開発用: 同じ写真のやりとりが残っていれば、Gemini を呼ばずにその応答を返します ([ocr] reuse_dumps)"
            );
        }
    } else if config.ocr.reuse_dumps && ocr.enabled() {
        tracing::warn!("[ocr] reuse_dumps は dump_dir が無いと効きません");
    }

    let google_login = GoogleLoginClient::from_env(config.server.public_url());
    if google_login.enabled() {
        tracing::info!("Google ログイン機能が有効です");
    } else {
        tracing::info!(
            "GOOGLE_OAUTH_CLIENT_ID/_SECRET が未設定のため、Google ログイン機能は無効です"
        );
    }

    let line_login = LineLoginClient::from_env(config.server.public_url());
    if line_login.enabled() {
        tracing::info!("LINE ログイン機能が有効です");
    } else {
        tracing::info!("LINE_LOGIN_CHANNEL_ID/_SECRET が未設定のため、LINE ログイン機能は無効です");
    }

    let apple_login =
        AppleLoginClient::from_env(config.server.public_url(), config.session.secure_cookie);
    match (apple_login.enabled(), apple_login.app_enabled()) {
        (true, true) => tracing::info!("Sign in with Apple (ウェブ・アプリ) が有効です"),
        (true, false) => tracing::info!(
            "Sign in with Apple (ウェブ) が有効です。APPLE_APP_BUNDLE_ID が未設定のため、アプリでは無効です"
        ),
        (false, true) => tracing::info!(
            "Sign in with Apple (アプリ) が有効です。APPLE_SERVICE_ID・server.public_url (https) ・session.secure_cookie のいずれかが揃わないため、ウェブでは無効です"
        ),
        (false, false) => tracing::info!(
            "APPLE_TEAM_ID/_KEY_ID/_PRIVATE_KEY と、APPLE_SERVICE_ID (と https の server.public_url・session.secure_cookie) または APPLE_APP_BUNDLE_ID が未設定のため、Sign in with Apple は無効です"
        ),
    }

    let stripe = StripeClient::from_env(&config.payments.stripe);
    if stripe.enabled() {
        tracing::info!("OCR 枠買い足し (Stripe) 機能が有効です");
        if stripe.tax_rate_missing() {
            tracing::warn!(
                "STRIPE_TAX_RATE_ID が未設定です。課税事業者になる 2026-12-01 から、買い足しの Checkout を作れなくなります"
            );
        }
    } else {
        tracing::info!(
            "STRIPE_SECRET_KEY/_WEBHOOK_SECRET/_OCR_TOPUP_PRICE_ID または [payments.stripe] が未設定のため、OCR 枠買い足し機能は無効です"
        );
    }

    let app_store = AppStoreClient::from_env();
    if app_store.enabled() {
        tracing::info!("アプリ内課金 (App Store) が有効です");
    } else {
        tracing::info!(
            "APP_STORE_ISSUER_ID/_KEY_ID/_PRIVATE_KEY または APPLE_APP_BUNDLE_ID が未設定のため、アプリ内課金は無効です"
        );
    }

    // 有効な機能に要る無料枠・単価・買い足しの量が無ければ、数え損なう・付け損なう前に止める。
    let ocr_costs = match config
        .ocr
        .costs(ocr.enabled(), stripe.enabled() || app_store.enabled())
    {
        Ok(costs) => costs,
        Err(message) => exit_with_log(log_guard, &message),
    };

    // セッション鍵の解決は DB に触れる前に行う: 設定不備 (secret の形式不正等) であれば、
    // DB 接続前に早期に失敗させたい。
    let session_key = match session::resolve_key(&config.session, &dirs.session_key_path()) {
        Ok(key) => key,
        Err(err) => exit_with_log(log_guard, &error_chain(&err)),
    };

    let server = Server {
        db_path: dirs.db_path(),
        config,
        session_key,
        ocr,
        google_login,
        line_login: Arc::new(line_login),
        apple_login: Arc::new(apple_login),
        stripe,
        app_store,
        ocr_costs,
    };

    let rt = tokio::runtime::Runtime::new().expect("failed to build tokio runtime");
    if let Err(err) = rt.block_on(server.run()) {
        exit_with_log(log_guard, &error_chain(err.as_ref()));
    }
    drop(log_guard);
}

/// サーバーの起動に要るもの。
struct Server {
    config: Config,
    db_path: PathBuf,
    session_key: Key,
    ocr: OcrService,
    google_login: GoogleLoginClient,
    line_login: Arc<LineLoginClient>,
    apple_login: Arc<AppleLoginClient>,
    stripe: StripeClient,
    app_store: AppStoreClient,
    ocr_costs: bp_carnet::config::OcrCosts,
}

impl Server {
    /// DB 接続からポートの bind までを済ませて待ち受け、`shutdown_signal` を受けたら停止する。
    /// 起動・待ち受けの失敗はエラーとして返し、プロセスは終了させない。終了はログを書き出す
    /// 呼び出し元に任せる (ここで終了すると、最後のログがファイルに残らない)。
    async fn run(self) -> Result<(), BoxError> {
        let Self {
            config,
            db_path,
            session_key,
            ocr,
            google_login,
            line_login,
            apple_login,
            stripe,
            app_store,
            ocr_costs,
        } = self;

        // lettre の接続プールは組み立てと同時にタスクを spawn するため、ランタイムの中で作る。
        // 設定の不備は DB に触れる前に止める。
        let mailer = Arc::new(Mailer::from_config(
            &config.mail,
            config.server.public_url(),
        )?);
        // メール内リンクのトークンは、それだけでパスワードを再設定できる秘密情報。http のままだと
        // 同じネットワーク上で盗み見られうるが、開発・テストでは http で使うため止めずに知らせる。
        if config.server.public_url().starts_with("http://") {
            tracing::warn!(
                "server.public_url が http のため、メール内リンクのトークンが暗号化されずに送られます。HTTPS で公開することを推奨します。"
            );
        }

        let pool = db::connect(&db_path).await?;
        db::migrate(&pool).await?;
        tracing::info!(db_path = %db_path.display(), "データベースに接続しました");

        let app = bp_carnet::build_app(
            pool.clone(),
            session_key,
            &config,
            bp_carnet::Services {
                ocr,
                google_login,
                line_login: line_login.clone(),
                apple_login: apple_login.clone(),
                mailer: mailer.clone(),
                stripe,
                app_store,
                ocr_costs,
            },
        )
        .await?;

        let addr = config.server.socket_addr();
        let listener = TcpListener::bind(addr)
            .await
            .map_err(|err| format!("サーバーのポート bind に失敗しました ({addr}): {err}"))?;
        tracing::info!("listening on http://{addr}");

        // build_app 内部でも同じ pool から SqliteStore を作るが、期限切れセッション行の
        // 削除タスクはテストからは不要なため main.rs 側だけで持つ。同じ pool・テーブルを
        // 指すインスタンスなので、build_app 側のストアと独立していても問題ない。
        // build_app が完了した (= session_store.migrate() 済み) 後に spawn することで、
        // 削除タスクの初回実行がテーブル作成より先に走る競合を避ける。
        let session_store = SqliteStore::new(pool.clone());
        let deletion_task = tokio::task::spawn(
            session_store
                .clone()
                .continuously_delete_expired(tokio::time::Duration::from_secs(60)),
        );
        // 猶予期間を過ぎたアカウントの物理削除 (docs/authentication.md)。
        // 放置アカウントの予告と削除予約 (docs/authentication.md)、失効したアプリのトークンの掃除 (docs/mobile-app.md) も同じ周期で回す。
        let purge_task = tokio::task::spawn(bp_carnet::account::purge_expired_periodically(
            pool,
            line_login,
            apple_login,
            mailer,
            config.inactivity.delete_after_days,
            config.session.expiry_days,
        ));

        // セッション削除タスクは自発的には終了しないため、停止と同時に abort する
        // (tower-sessions-sqlx-store 0.15 README の SQLite 例に準拠)。
        let abort_handles = [deletion_task.abort_handle(), purge_task.abort_handle()];
        // 接続元アドレスは、プロキシのヘッダーを信頼してよいかの判定に使う (`src/forwarded.rs`)。
        let app = app.into_make_service_with_connect_info::<std::net::SocketAddr>();
        axum::serve(listener, app)
            .with_graceful_shutdown(async move {
                shutdown_signal().await;
                for handle in abort_handles {
                    handle.abort();
                }
            })
            .await
            .map_err(|err| format!("サーバーが異常終了しました: {err}"))?;

        // shutdown で abort 済みなら JoinError::is_cancelled() になるので
        // エラーとしては扱わない。continuously_delete_expired 自体が返す内側の Result も
        // 確認する (DB エラー等で削除ループが止まった場合、外側の JoinError だけでは
        // 検知できない)。
        match deletion_task.await {
            Ok(Err(err)) => {
                tracing::error!(error = %error_chain_line(&err), "セッション削除タスクが異常終了しました");
            }
            Err(err) if !err.is_cancelled() => {
                tracing::error!(error = %error_chain_line(&err), "セッション削除タスクが異常終了しました");
            }
            Ok(Ok(())) | Err(_) => {}
        }

        // こちらはループ内でエラーを処理しているため、確認するのは JoinError だけでよい。
        if let Err(err) = purge_task.await
            && !err.is_cancelled()
        {
            tracing::error!(error = %error_chain_line(&err), "アカウント削除タスクが異常終了しました");
        }
        Ok(())
    }
}

/// Ctrl-C (SIGINT) または SIGTERM を受けたら返る。`axum::serve(..).with_graceful_shutdown(..)` に
/// 渡すことで、進行中のリクエスト (OCR・インポート等) の完了を待ってから終了できる。
async fn shutdown_signal() {
    let ctrl_c = async {
        tokio::signal::ctrl_c()
            .await
            .expect("failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("failed to install signal handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => tracing::info!("Ctrl-C を受信しました。graceful shutdown を開始します"),
        _ = terminate => tracing::info!("SIGTERM を受信しました。graceful shutdown を開始します"),
    }
}

/// ログを初期化する前の失敗を、標準エラー出力に書き出して終了する。
fn exit_with_error(err: &(dyn std::error::Error + 'static)) -> ! {
    eprintln!("{}", error_chain(err));
    std::process::exit(1);
}

/// ログを初期化した後の失敗を、ログと標準エラー出力の両方に書き出し、ログを書き出し終えてから終了する。
fn exit_with_log(log_guard: WorkerGuard, message: &str) -> ! {
    tracing::error!("{message}");
    eprintln!("{message}");
    drop(log_guard);
    std::process::exit(1);
}
