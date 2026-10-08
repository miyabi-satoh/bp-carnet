# データモデル

DB は SQLite 1ファイル。スキーマの正は `migrations/` (sqlx のマイグレーション。起動時に自動で適用する)。ここでは、テーブルごとの役割と、スキーマを読むだけでは分からない約束を書く。

## 全体の約束

- 接続の設定 (`src/db.rs`)
  - WAL・`synchronous = NORMAL`・`foreign_keys = ON`・`secure_delete = ON` (消した行の中身を 0 で上書きする)。
  - 外部キーの `ON DELETE CASCADE` / `SET NULL` は、この `foreign_keys = ON` を前提にしている。
- 日時は TEXT に UTC の RFC3339 文字列で持つ。期限・期間の判定は文字列の大小で比べる。
  - `bp_records.measured_at` は秒精度 (`2026-09-06T22:15:00Z`)。書き込みは `src/local_time.rs` を通す。
  - ほかの日時はミリ秒精度 (`2026-09-06T22:15:00.000Z`)。SQL では `strftime('%Y-%m-%dT%H:%M:%fZ', ...)`、Rust では `payments::format_sql` で作る。
  - 大小で比べるので、同じ列に秒とミリ秒の形を混ぜない。
- 真偽値は INTEGER の 0 / 1。
- 金額は INTEGER の円 (`*_yen`) か、1/1000 円 (`*_milli_yen`)。読み取りの費用は1回が数円未満なので、累計や残りは milli-yen で持つ。
- ユーザーのタイムゾーンでの日付・時刻は DB に持たない。API の入出力でだけ `users.timezone` で変換する。
- セッションのテーブルは `tower-sessions-sqlx-store` が作る (`migrations/` には無い)。

## テーブル

| テーブル | 役割 | ユーザーを消したとき |
|---|---|---|
| `users` | アカウントと個人設定 | — |
| `bp_records` | 血圧の記録 | 一緒に消える (CASCADE) |
| `oauth_identities` | Google・LINE・Apple との連携 | 一緒に消える (CASCADE) |
| `app_tokens` | モバイルアプリのログイン | 一緒に消える (CASCADE) |
| `email_tokens` | メールのリンク (確認・再設定・変更) | 一緒に消える (CASCADE) |
| `ocr_quota_grants` | 読み取りの枠の購入1件ごとの記録 | 残る (`user_id` が NULL になる) |
| `ocr_usage_records` | 読み取りに使った額の記録 | 残る (ユーザーの列を持たない) |
| `stripe_webhook_events` | Stripe の Webhook の受領記録 | 関係しない |
| `admin_audit_log` | 管理者の閲覧・操作の記録 | 残る (外部キーにしていない) |

### `users`

- `id` は `AUTOINCREMENT`。消したユーザーの id を使い回すと、残ったセッションが別人として通るため。
- `username` は、メールアドレスでも任意の ID でも、書式で分けずに常に小文字にして保存・照合する。
  - 列と UNIQUE 制約はそのままで、アプリ側の `auth::normalize_username` で担保する。username を検索条件に使う箇所は、すべてこの関数を通す。
  - 小文字化は ASCII だけ (`to_ascii_lowercase`)。既存行を変換したマイグレーションが使った SQLite の `lower()` と結果をそろえるため。
- `password_hash` は NOT NULL。パスワードを持たないアカウントは、絶対に一致しないハッシュを入れ、`password_usable = 0` にする。
- `session_generation`: パスワードの変更などで増やす。保存した世代とずれたセッション・`app_tokens` は無効 (→ docs/authentication.md)。
- `email_verified = 0` のアカウントは、確認のリンクの期限が切れると消す (`signup::purge_expired_unverified`)。
- 削除の予約
  - `deletion_scheduled_at` が NULL でなければ削除の猶予期間中。予約から30日後 (`account::DELETION_GRACE_DAYS`) を入れ、過ぎたら定期タスクが行ごと消す。
  - `deletion_origin` は予約の起点 (`user` / `admin` / `inactive`)。本人と自動退会が起点の予約は、本人のログイン・アクセスで取り消す。管理者が起点の予約は、管理者だけが取り消す。
  - `last_seen_at` は最終アクセス。NULL は `created_at` として扱う。`inactivity_notice_days` は最後に送った自動退会の予告 (30 / 7 / 1)。
- 個人設定
  - `timezone` は IANA のタイムゾーン名。記録の日付の区切り・朝/夜の判定・CSV の日時に使う。`timezone_auto = 1` の間はブラウザのタイムゾーンで更新する。
  - `morning_start_min` などは、その日の 0:00 からの分 (0〜1440)。開始を含み、終了を含まない。
- 読み取り (OCR)
  - `ocr_budget_yen`: 無料の枠の上限 (円)。NULL は設定の既定値、負の値は無制限、0 は使えない。
  - `ocr_spent_milli_yen`: 無料の枠から使った累計。
  - `ocr_consented_at`: 写真を外部の API に送ることへの同意。NULL は未同意。
  - `apple_app_account_token`: アプリ内課金の appAccountToken (小文字の UUID)。初めて買うときに作る。
- `terms_version`・`terms_agreed_at`: 同意した利用規約・プライバシーポリシーの版と日時。
- `role` は `user` / `admin`。`frozen = 1` は管理者による凍結。

### `bp_records`

- `measured_at` は UTC の秒精度。入力は分単位なので、秒は常に `00`。
- 記録が「どの日のものか」は、読むときの `users.timezone` で決まる。タイムゾーンを変えると、日付の境目に近い記録の日付が変わる。
- `pulse` は NULL 可 (未入力)。`memo` は空文字列が既定で、NULL にしない。
- 値域 (収縮期 60〜260・拡張期 30〜180・脈拍 30〜220・収縮期 > 拡張期) は DB の制約でなく、`src/validation.rs` で全経路に掛ける。
- `version`: 直すたびに増やす版番号。直す・消す・取り込むときに、画面を開いた時点の値を送り、合わなければ止める (楽観的な排他)。
- 消すときは行ごと消す (論理削除は無い)。

### `oauth_identities`

- キーは `(provider, subject)`。`provider` は `google` / `line` / `apple`、`subject` は各社の `sub`。変わりうる `email` はキーにしない。
- `refresh_token` (LINE・Apple) と `access_token` (LINE) は、アカウントを消すときに連携を取り消すためだけに持つ。暗号化して入れる (`src/token_cipher.rs`)。

### `app_tokens` と `email_tokens`

- どちらもトークン本体は持たず、ハッシュ (`token_hash`) だけを持つ。
- `app_tokens`: `last_used_at` から `[session] expiry_days` 日 (既定 30) で失効する。`session_generation` が `users` とずれても無効。
- `email_tokens`
  - `purpose` は `verify_email` / `reset_password` / `change_email`。
  - `UNIQUE (user_id, purpose)`: 発行し直すと上書きし、前のリンクは使えなくなる。
  - 期限は `verify_email` が24時間、ほかは1時間。`used_at` を入れたら使用済み。
  - `new_email` は `change_email` の変更先。ほかの用途では NULL。

### `ocr_quota_grants` と `ocr_usage_records`

読み取りの枠と、使った額の記録。何を持つかだけを書く。枠の使い方と決済は docs/ocr.md・docs/payments.md。

- `ocr_quota_grants`: 購入1件につき1行。
  - `granted_milli_yen` (付与した量)・`remaining_milli_yen` (残り)・`price_yen` (払われた額)・`purchased_at`。
  - 決済の識別子は `stripe_checkout_session_id` か `apple_transaction_id` のどちらか一方だけ (CHECK)。`apple_*` の列はアプリ内課金の行だけが持つ。
  - `revoked_milli_yen` が NULL でなければ、返金などで取り消した購入。
  - `domestic` は買い手が国内か。分からなければ NULL。
  - ユーザーを消すと `user_id` が NULL になり、トリガー `ocr_quota_grants_clear_orphaned_remaining` が残りを 0 にする。持ち主の無い行は、保存期間を過ぎると定期タスクが消す。
- `ocr_usage_records`: 読み取りで枠を使うたびに1行。
  - `kind` は `paid` (買った枠) / `sandbox` (Apple の Sandbox で買った枠) / `free` (無料の枠)。
  - `grant_id` は減らした購入の行。購入の行より長く残ることがあるので外部キーにしない。`domestic` は購入の行から写す。
  - `used_milli_yen` は使った量、`sales_milli_yen` はそれを売値に直した額 (`paid` だけ。ほかは 0)。
  - ユーザーの列は持たない。保存期間を過ぎた行は定期タスクが消す。

### `stripe_webhook_events`

- `event_id` を主キーにし、同じイベントの再送を一度だけ処理する。`status` は `received` / `processed` / `failed`。

### `admin_audit_log`

- 管理者が誰に何をしたか (`action`) を残す。値は `src/audit.rs` の `Action`。
- 対象のユーザーを消した後も残すため、`admin_user_id`・`target_user_id` は外部キーにせず、id をそのまま持つ。
- 状態を変える操作は、その操作と同じトランザクションで記録する。
