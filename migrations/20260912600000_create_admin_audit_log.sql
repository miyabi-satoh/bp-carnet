-- 管理者による閲覧・操作の記録 (docs/design.md 2.5・10章)。
--
-- ユーザーが削除された後も記録自体は残す必要があるため、外部キー制約は付けず id をそのまま
-- 保持する。本人による自分のデータへの操作は対象外。
CREATE TABLE admin_audit_log (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    admin_user_id INTEGER NOT NULL,
    target_user_id INTEGER NOT NULL,
    action TEXT NOT NULL,
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);

-- 「このユーザーに何が行われたか」を追う用途を想定した検索用インデックス。
CREATE INDEX idx_admin_audit_log_target_user_id ON admin_audit_log (target_user_id, created_at);
