//! 管理者による閲覧・操作の記録 (`admin_audit_log`、docs/authentication.md)。
//!
//! 記録するのは「誰が・いつ・誰に対して・何を」行ったか。対象ユーザーが定まらない操作
//! (ユーザー一覧の取得など) は対象外で、`target_user_id` を持てる操作だけがここを通る。
//!
//! 操作は、状態が変わったかどうかによらず受け付けるたびに記録する (同じ状態への凍結、
//! 予約済みのユーザーへの削除も含む)。拒否した操作 (403・404・409 等) は記録しない。

use sqlx::SqliteExecutor;

/// 記録する操作の種類。`admin_audit_log.action` に保存する文字列に対応する。
/// バリアントは実際に記録する箇所ができた時点で追加する (投機的に増やさない)。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Freeze,
    Unfreeze,
    ChangeOcrLimit,
    Delete,
    CancelDeletion,
    Create,
    ResetPassword,
    ViewAccount,
    ViewRecords,
}

impl Action {
    fn as_str(self) -> &'static str {
        match self {
            Self::Freeze => "freeze",
            Self::Unfreeze => "unfreeze",
            Self::ChangeOcrLimit => "change_ocr_limit",
            Self::Delete => "delete",
            Self::CancelDeletion => "cancel_deletion",
            Self::Create => "create",
            Self::ResetPassword => "reset_password",
            Self::ViewAccount => "view_account",
            Self::ViewRecords => "view_records",
        }
    }
}

/// 操作を1件記録する。`executor` はプール本体・トランザクションのどちらでも渡せる。
///
/// 変更を伴う操作は、同じトランザクションで記録すること。別々に書くと、操作だけが成立して
/// 記録が残らない状態が生まれる。閲覧は読み取った後に単独で記録し、記録できなければ
/// 読んだ内容を返さない (返さなければ閲覧は成立していないため、トランザクションは要らない)。
pub async fn record<'e, E: SqliteExecutor<'e>>(
    executor: E,
    admin_user_id: i64,
    target_user_id: i64,
    action: Action,
) -> Result<(), sqlx::Error> {
    let action = action.as_str();
    sqlx::query!(
        "INSERT INTO admin_audit_log (admin_user_id, target_user_id, action) VALUES (?, ?, ?)",
        admin_user_id,
        target_user_id,
        action
    )
    .execute(executor)
    .await?;
    Ok(())
}
