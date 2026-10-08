//! コマンドライン引数の処理 (サーバー起動以外のサブコマンド)。

use std::fmt::Display;
use std::path::Path;

use crate::auth::{PasswordReset, Role};
use crate::config::{AppDirs, Config};
use crate::error_chain;
use crate::validation::{self, UsernameError};

/// コマンドライン引数を処理する。処理して呼び出し元が即終了すべきなら `true` を返す。
pub fn handle_args() -> bool {
    let args: Vec<String> = std::env::args().collect();
    match args.get(1).map(String::as_str) {
        None => return false,
        Some("-v") | Some("--version") => {
            println!("v{}", env!("CARGO_PKG_VERSION"));
        }
        Some("--openapi") => {
            print_openapi();
        }
        Some("--create-user") => {
            create_user(&args[2..]);
        }
        Some("--reset-password") => {
            reset_password(&args[2..]);
        }
        Some("-h") | Some("--help") => {
            print_usage();
        }
        Some(other) => {
            eprintln!("unknown option: {other}");
            print_usage();
            std::process::exit(1);
        }
    }
    true
}

fn print_usage() {
    eprintln!(
        "使い方: bp-carnet [オプション]\n\
         \n\
         オプション無しでサーバーを起動する。\n\
         \n\
         -v, --version                      バージョンを表示する\n\
         --openapi                          OpenAPI 仕様 (JSON) を標準出力に書き出す\n\
         --create-user <username> [--admin] ユーザーを作成する (パスワードは対話入力、--admin で管理者)\n\
         --reset-password <username>        パスワードを再設定する (パスワードは対話入力)\n\
         -h, --help                         このヘルプを表示する"
    );
}

/// 標準エラー出力に書き出して終了する。
fn exit_with(message: impl Display) -> ! {
    eprintln!("{message}");
    std::process::exit(1);
}

/// OpenAPI スキーマを JSON で標準出力に書き出す。
/// `frontend/package.json` の `generate:api-types` (openapi-typescript) が `../openapi.json` を
/// 読みに行くため、`bp-carnet --openapi > openapi.json` として使う想定。
fn print_openapi() {
    match crate::api::openapi().to_pretty_json() {
        Ok(json) => println!("{json}"),
        Err(err) => exit_with(format!("OpenAPI スキーマの出力に失敗しました: {err}")),
    }
}

/// `--create-user` に続く引数を、ユーザー名と権限に解釈する。ユーザー名1つと、任意の
/// `--admin` だけを受け付け、それ以外 (ユーザー名の欠落・重複・未知のオプション) は `None`。
fn parse_create_user_args(args: &[String]) -> Option<(&String, Role)> {
    let mut username: Option<&String> = None;
    let mut role = Role::User;
    for arg in args {
        match arg.as_str() {
            "--admin" => role = Role::Admin,
            other if other.starts_with('-') => return None,
            _ => {
                if username.replace(arg).is_some() {
                    return None;
                }
            }
        }
    }
    Some((username?, role))
}

/// ユーザーIDが使えない理由を、対話入力の利用者向けの文言にする (API 経路は開発者向けの
/// 説明だけを返し、表示文言は frontend が持つ。CLI にはその frontend が無い)。
fn username_error_message(error: UsernameError) -> String {
    match error {
        UsernameError::Empty => "ユーザーIDを指定してください".to_string(),
        UsernameError::TooLong => format!(
            "ユーザーIDは{}文字以内にしてください",
            validation::USERNAME_MAX_CHARS
        ),
        UsernameError::InvalidCharacter => {
            "空白や制御文字を含まないユーザーIDを指定してください".to_string()
        }
    }
}

/// `--reset-password` に続く引数を、ユーザー名に解釈する。ユーザー名1つだけを受け付ける。
fn parse_reset_password_args(args: &[String]) -> Option<&String> {
    match args {
        [username] if !username.starts_with('-') => Some(username),
        _ => None,
    }
}

fn role_label(role: Role) -> &'static str {
    match role {
        Role::Admin => "管理者ユーザー",
        Role::User => "ユーザー",
    }
}

/// 設定を読む。パスワードを入力させる前に呼ぶ: 設定が壊れていて失敗するなら、入力を求める前に
/// 終わらせる。
fn load_config() -> (AppDirs, Config) {
    let dirs = AppDirs::resolve().unwrap_or_else(|err| exit_with(error_chain(&err)));
    let config = Config::load(&dirs).unwrap_or_else(|err| exit_with(error_chain(&err)));
    (dirs, config)
}

/// 新しいパスワードを確認付きで対話入力させ、ハッシュにして返す。
/// パスワードは引数や環境変数では受け取らない (シェル履歴やプロセス一覧に残るため)。
fn prompt_new_password_hash(config: &Config) -> String {
    let prompt = |label: &str| {
        rpassword::prompt_password(label).unwrap_or_else(|err| {
            exit_with(format!(
                "パスワードの入力に失敗しました: {}",
                error_chain(&err)
            ))
        })
    };
    let password = prompt("パスワード: ");
    if let Err(err) = crate::auth::validate_password(&config.password, &password) {
        exit_with(err);
    }
    if prompt("パスワード (確認): ") != password {
        exit_with("パスワードが一致しません");
    }

    crate::auth::hash_password(&password).unwrap_or_else(|err| exit_with(error_chain(&err)))
}

/// ユーザーを作成する。端末から最初の管理者を作る手段でもある。
fn create_user(args: &[String]) {
    let Some((username, role)) = parse_create_user_args(args) else {
        exit_with("使い方: bp-carnet --create-user <username> [--admin]");
    };
    // 実際に保存される名前 (小文字) をメッセージに出すため、ここでも正規化しておく。
    let username = crate::auth::normalize_username(username);
    // 管理画面からの作成 (`api::admin`) と同じ条件で弾く。見えない文字を含むIDを作ると、
    // 本人がログインのたびに「入力したはずのIDで入れない」状態になる。
    if let Err(err) = validation::validate_username(&username) {
        exit_with(username_error_message(err));
    }

    let (dirs, config) = load_config();
    let hash = prompt_new_password_hash(&config);

    let db_path = dirs.db_path();

    let result = {
        let rt = tokio::runtime::Runtime::new().expect("failed to build tokio runtime");
        rt.block_on(async {
            let pool = crate::db::connect(&db_path).await?;
            crate::db::migrate(&pool).await?;
            // 管理画面からの作成 (`api::admin`) と同じく、未確認のアカウントの破棄と作成を
            // 1つのトランザクションで行う。
            let mut tx = pool.begin().await?;
            crate::auth::discard_unverified_user(&mut *tx, &username).await?;
            crate::auth::create_user(
                &mut *tx,
                &username,
                crate::auth::NewPassword::Usable(&hash),
                role,
            )
            .await?;
            tx.commit().await?;
            Ok::<(), crate::db::Error>(())
        })
    };

    match result {
        Ok(()) => println!(
            "{} '{username}' を作成しました ({})",
            role_label(role),
            db_path.display()
        ),
        Err(crate::db::Error::Sqlx(sqlx::Error::Database(db_err)))
            if db_err.is_unique_violation() =>
        {
            exit_with(format!("ユーザー '{username}' は既に存在します"));
        }
        Err(err) => exit_with(format!(
            "ユーザーの作成に失敗しました: {}",
            error_chain(&err)
        )),
    }
}

/// パスワードを再設定する (docs/authentication.md)。管理画面の再設定と違い自分自身も対象にでき、
/// 監査ログにも残さない (このマシンに触れる人は DB を直接書き換えられるため)。
fn reset_password(args: &[String]) {
    let Some(username) = parse_reset_password_args(args) else {
        exit_with("使い方: bp-carnet --reset-password <username>");
    };
    let username = crate::auth::normalize_username(username);
    let (dirs, config) = load_config();
    let db_path = dirs.db_path();
    // `db::connect` は DB が無ければ作る。データディレクトリの解決先がずれた状態で実行したときに、
    // 空の DB を残さない (後でそこでサーバーを起動すると、元のデータの無い空の DB で動いてしまう)。
    if !db_path.exists() {
        exit_with(format!(
            "DB が見つかりません ({})。サーバーと同じ環境変数・実行ユーザーで実行してください",
            db_path.display()
        ));
    }

    let rt = tokio::runtime::Runtime::new().expect("failed to build tokio runtime");
    let pool = rt
        .block_on(async {
            let pool = crate::db::connect(&db_path).await?;
            crate::db::migrate(&pool).await?;
            Ok::<_, crate::db::Error>(pool)
        })
        .unwrap_or_else(|err| exit_with(format!("DB を開けませんでした: {}", error_chain(&err))));

    // パスワードを入力させる前に確かめる: 設定できない相手なら、入力を求める前に終わらせる。
    // 再設定 (`auth::reset_password_hash`) が断る条件と同じものを、先に読んで確かめる。
    let lookup = rt.block_on(async {
        let Some(user_id) = crate::auth::find_verified_user_id(&pool, &username).await? else {
            return Ok(Err(PasswordReset::NotFound));
        };
        let profile = crate::auth::load_profile(&pool, user_id).await?;
        Ok::<_, sqlx::Error>(if profile.password_usable {
            Ok(user_id)
        } else {
            Err(PasswordReset::NoPassword)
        })
    });
    let user_id = match lookup {
        Ok(Ok(user_id)) => user_id,
        Ok(Err(reason)) => exit_unresettable(&username, &db_path, reason),
        Err(err) => exit_with(format!(
            "ユーザーの検索に失敗しました: {}",
            error_chain(&err)
        )),
    };

    let hash = prompt_new_password_hash(&config);

    let result = rt.block_on(async {
        let mut conn = pool.acquire().await?;
        crate::auth::reset_password_hash(&mut conn, user_id, &hash).await
    });
    match result {
        Ok(PasswordReset::Done) => {
            let changed_at = jiff::Timestamp::now();
            println!(
                "ユーザー '{username}' のパスワードを再設定しました。ログイン中の端末はすべてログアウトされます"
            );
            rt.block_on(notify_password_reset(&pool, &config, user_id, changed_at));
        }
        // `NotFound` は、入力の間に削除されたとき。
        Ok(reason) => exit_unresettable(&username, &db_path, reason),
        Err(err) => exit_with(format!(
            "パスワードの再設定に失敗しました: {}",
            error_chain(&err)
        )),
    }
}

/// サーバーと同じメールの設定で、パスワードが変わったことを本人に知らせる。再設定は済んでいるので、
/// メールの設定の誤りでは失敗にせず、知らせられなかったことだけを伝える。
async fn notify_password_reset(
    pool: &sqlx::SqlitePool,
    config: &crate::config::Config,
    user_id: i64,
    changed_at: jiff::Timestamp,
) {
    match crate::mail::Mailer::from_config(&config.mail, config.server.public_url()) {
        Ok(mailer) => {
            let origin = crate::password_notice::Origin::Terminal;
            if let Err(err) =
                crate::password_notice::send(pool, &mailer, user_id, origin, changed_at).await
            {
                eprintln!(
                    "本人に知らせるメールを送れませんでした: {}",
                    error_chain(&err)
                );
            }
        }
        Err(err) => eprintln!(
            "メールの設定を読めないため、本人に知らせるメールは送っていません: {}",
            error_chain(&err)
        ),
    }
}

/// パスワードを再設定できない理由を伝えて終了する。入力の前の確認と、再設定の結果の両方から呼ぶ。
fn exit_unresettable(username: &str, db_path: &Path, reason: PasswordReset) -> ! {
    match reason {
        PasswordReset::NoPassword => exit_with(format!(
            "ユーザー '{username}' は Google ログインだけで使うアカウントのため、パスワードを設定できません"
        )),
        PasswordReset::NotFound => exit_with(format!(
            "ユーザー '{username}' は存在しません ({})",
            db_path.display()
        )),
        PasswordReset::Done => unreachable!("再設定できた結果は渡さない"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(args: &[&str]) -> Vec<String> {
        args.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn create_user_args_default_to_normal_user() {
        let args = args(&["kyoko"]);
        let (username, role) =
            parse_create_user_args(&args).expect("ユーザー名だけなら解釈できるべき");
        assert_eq!(username, "kyoko");
        assert_eq!(role, Role::User);
    }

    #[test]
    fn create_user_args_accept_admin_flag_in_any_order() {
        for raw in [["kyoko", "--admin"], ["--admin", "kyoko"]] {
            let args = args(&raw);
            let (username, role) =
                parse_create_user_args(&args).expect("--admin は前後どちらでも解釈できるべき");
            assert_eq!(username, "kyoko");
            assert_eq!(role, Role::Admin);
        }
    }

    #[test]
    fn reset_password_args_accept_only_a_single_username() {
        let valid = args(&["kyoko"]);
        assert_eq!(parse_reset_password_args(&valid), Some(&valid[0]));
        for raw in [
            vec![],
            vec!["kyoko", "mika"],
            vec!["--admin"],
            vec!["kyoko", "--admin"],
        ] {
            let args = args(&raw);
            assert!(
                parse_reset_password_args(&args).is_none(),
                "{raw:?} は解釈できないべき"
            );
        }
    }

    #[test]
    fn username_error_message_names_the_limit_for_a_too_long_id() {
        let message = username_error_message(UsernameError::TooLong);
        assert!(
            message.contains(&validation::USERNAME_MAX_CHARS.to_string()),
            "上限を文言に含めるべき: {message}"
        );
    }

    #[test]
    fn create_user_args_reject_unusable_input() {
        for raw in [vec![], vec!["kyoko", "mika"], vec!["kyoko", "--unknown"]] {
            let args = args(&raw);
            assert!(
                parse_create_user_args(&args).is_none(),
                "{raw:?} は解釈できないべき"
            );
        }
    }
}
