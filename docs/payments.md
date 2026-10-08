# 読み取りの枠の購入 (決済)

写真の読み取り (docs/ocr.md) の枠を買い足す仕組み。
ウェブは Stripe Checkout の単発決済、iPhone・iPad のアプリは App Store のアプリ内課金 (消耗型) で売り、どちらも同じ「購入ごとの枠」を足す。

## 置き場所

- `src/payments/mod.rs`: Checkout の作成・Webhook の処理・アプリ内課金の取引と通知の処理
- `src/payments/stripe.rs`: Stripe API の呼び出しと Webhook の署名検証
- `src/payments/app_store.rs`: App Store Server API の呼び出しと通知の読み取り
- `src/payments/ocr_quota.rs`: 購入ごとの枠 (`ocr_quota_grants`) の付与・消費・取り消し
- `src/api/payments.rs`: HTTP の口
- `frontend/src/lib/app-store-purchase.ts`・`mobile/plugins/app-store-purchase`: アプリ側の StoreKit 2 の呼び出し

## 購入ごとの枠

- 1回の購入で、環境変数 `OCR_TOPUP_GRANT_YEN` (円、無ければ `[ocr] topup_grant_yen`) の量の枠を1行足す。Stripe かアプリ内課金が有効なのに無ければ、起動しない。買い手が払った額は `price_yen` に残す。
- 枠は失効しない。読み取りでは買い足した枠を購入の順 (`id`) に先に使い、無料の枠は最後 (→ docs/ocr.md「枠と上限」)。
- 取り消した行は `revoked_milli_yen` が `NULL` でない。取り消したときの残りをここへ移し、残り (`remaining_milli_yen`) は 0 にする。
- アカウントを消すと、行の `user_id` を `NULL` にして残し、トリガーが残りを 0 にする。決済の ID・購入日時・金額・取り消したかだけが残り、保存期間を過ぎると定期タスクが消す (`purge_orphaned`)。
- 枠の残っているユーザーは、放置アカウントの自動退会の対象にしない (`src/inactivity.rs`)。
- 買い手が国内かを購入のときに `domestic` に残す (Stripe は Checkout の住所の国が `JP` か、Apple は取引の `storefront` が `JPN` か。分からなければ `NULL`)。
- 使った額の記録 (`ocr_usage_records`) は、買い足した枠の分に `sales_milli_yen` (`price_yen` × 使った量 ÷ 付与した量、切り捨て) を付ける。Apple の Sandbox の枠の分は `kind = 'sandbox'` で額 0。

## 有効化の条件

- Stripe: 次がすべてそろったときだけ有効 (`StripeClient::from_env`)。
  - `[payments.stripe] enabled = true`、`checkout_success_url`・`checkout_cancel_url` が空でない
  - 環境変数 `STRIPE_SECRET_KEY`・`STRIPE_WEBHOOK_SECRET`・`STRIPE_OCR_TOPUP_PRICE_ID`
- アプリ内課金: 環境変数 `APP_STORE_ISSUER_ID`・`APP_STORE_KEY_ID`・`APP_STORE_PRIVATE_KEY` (In-App Purchase の鍵) と `APPLE_APP_BUNDLE_ID` がそろい、秘密鍵が PKCS#8 として読めたときだけ有効。
- 無効なら、それぞれの口は 503 (`payments_disabled`)。
- `GET /ocr/status` は、ウェブ向けに `topupAvailable` (Stripe が有効)、アプリ向けに `appStoreTopupAvailable` (アプリ内課金が有効) を返す。どちらも無料の枠が無制限のユーザーには `false`。
- 鍵の用意は DEVELOPMENT.md。

## Stripe Checkout

- `POST /payments/ocr-topup/checkout` が Checkout Session を作り、`checkoutUrl` を返す。
  - 金額・Price・ユーザーをクライアントから受け取らない。サーバーの Price ID とログイン中のユーザーだけで作る。
  - `metadata[user_id]` と製品の印 `metadata[product]=bp-carnet` を付ける。
  - メール確認済みのアドレスがあれば `customer_email` に事前入力する。
  - `managed_payments[enabled]=false` と、カードの明細の表記の suffix (英字・漢字・カナ) を Session ごとに渡す。
  - `TAX_INCLUDED_FROM` から、項目に税率 `STRIPE_TAX_RATE_ID` を付ける。その日時を過ぎて税率が無ければ Checkout を作らない (503)。
- 枠を足すのは Webhook だけ。`success_url` への戻りでは足さない。
- `GET /payments/ocr-topup/result?sessionId=...` は、戻り先の画面が Webhook の反映を待つための口。
  - 付与済みなら `succeeded` (その後に返金されていれば `failed`)、期限切れ・非同期の支払いの失敗・返金済みなら `failed`、それ以外は `processing`。
  - 他人の Session なら 404。

## Stripe Webhook (`POST /payments/stripe/webhook`)

- 認証と CSRF の対象外。`Stripe-Signature` を生の本文で HMAC-SHA256 検証し、時刻のずれは 300 秒まで (違えば 400)。
- イベント ID で冪等にする (`stripe_webhook_events`)。`processed` なら何もしない。途中で失敗したもの (`received`・`failed`) は再送でやり直す。
- 扱うイベント:
  - `checkout.session.completed`・`checkout.session.async_payment_succeeded`: 枠を足す。
  - `checkout.session.async_payment_failed`: 足さず、Session ID だけ記録する (結果の口が失敗と判定するため)。
  - `charge.refunded`・`charge.dispute.created`: PaymentIntent から Session を引き、その購入の残りを取り消す (`revoke_unspent`)。使った分は戻さない。
  - それ以外は記録して 200。
- 枠を足す前の検証 (`grant_ocr_topup`):
  - `mode = payment`・`payment_status = paid` でなければ何もしない。
  - `metadata[product]` がほかの製品のものなら何もしない (同じ Stripe のアカウントにほかの製品の Webhook も届くため)。印が無いものはこのアプリの Session として検証に進む。
  - イベントの中身を信じず、Session を Stripe API から取り直して (`expand[]=line_items`)、支払い済み・金額 (`TOPUP_PRICE_YEN`、`jpy`)・項目が1件で Price と数量 1 が設定どおりか・`metadata[user_id]` を確かめる。合わなければログに出して足さない。
  - 購入日時はイベントの `created` (配送の遅れを持ち込まない)。
- 返金と付与の競合: 返金のイベントが付与より先に届くことがある。返金済みかの確認と付与、取り消しと「処理済み」の記録を、それぞれ同じ `BEGIN IMMEDIATE` のトランザクションで行い、返金済みの決済に枠を残さない。
- 同じ Session は `stripe_checkout_session_id` の一意制約で1回しか足さない。

## アプリ内課金

- 商品は製品 ID `com.amiiby.bpcarnet.ocr_topup` (`app_store::PRODUCT_ID`) の消耗型1つ。
- 取引とアカウントの結び付け:
  - アカウントごとに固定の UUID (`users.apple_app_account_token`、小文字) を、購入オプションの `appAccountToken` に渡す。
  - アプリは購入の前に `POST /payments/apple/account-token` で受け取る。無ければ作り、`NULL` のときだけ書いて読み直すので、2台から同時に作っても1つに決まる。
  - サーバーは、取引の `appAccountToken` の持ち主に枠を足す。送ってきたユーザーではない (他人の取引 ID を送られても、枠は持ち主にしか付かない)。
- 取引の検証 (`POST /payments/apple/transactions`):
  - アプリは購入の後、取引 ID と環境 (`Transaction.environment`) だけを送る。JWS (`jwsRepresentation`) は送らず、サーバーも信じない。
  - サーバーは App Store Server API の Get Transaction Info で Apple から取引を取り直し、それだけで決める。JWS の証明書の連なりは確かめない (TLS で Apple から直接取ったものだけを使うため)。
  - アプリが添えた環境で引き、見つからなければもう一方の環境でも引く。`Xcode` (StoreKit のローカルのテスト) は引かずに `rejected`。
  - 取引 ID が32桁以内の数字の列でなければ、Apple に送らずに `rejected`。
  - 足すのは `bundleId` がアプリの ID・種類が `Consumable`・製品 ID が上の商品・`quantity` が 1 の取引だけ。購入日時は `purchaseDate`。
  - API の JWT (ES256) は呼ぶたびに作る。`bid` は `APPLE_APP_BUNDLE_ID`。
  - ユーザーごとに送る回数の上限があり、超えると 429。
- 応答の `status` と、アプリが取引を終える (`finish()`) か:
  - `granted` (足した)・`already_granted` (足し済み)・`rejected` (ずっと足さない): 終える。
  - 503 (`app_store_unavailable`: 通信の失敗・Apple の 401・429・5xx・両方の環境で見つからない) や 429: 終えない。
- 取りこぼし: アプリはログイン後に1回、終えていない取引 (`Transaction.unfinished`) を送り、以後は届く取引 (`Transaction.updates`) を送る。終えていない取引は、アプリに戻るたび (`resume`) にも送る。

## App Store Server Notifications V2 (`POST /payments/apple/notifications`)

- 認証と CSRF の対象外。本番・Sandbox の通知を同じ口で受ける。
- 送り元の確かめ方:
  - 接続元が Apple の範囲 (`17.0.0.0/8`) でなければ 403。黙って 200 にすると、プロキシの設定の誤りで本物の通知を落としても Apple が再送せず、気づけないため。
  - 接続元は `forwarded::ClientIp` で取る。プロキシの後ろでは `[server] proxy_headers` の設定が要る。
- 署名付きの本文は、取引 ID と種類を知るためだけに読み、署名は確かめない。何をするかは取り直した取引だけで決めるので、偽の通知で枠は増えも減りもしない。
- `ONE_TIME_CHARGE`・`REFUND`・`REFUND_REVERSED` だけを扱う。ほかの種類、`bundleId` が違うもの、環境が `Production`・`Sandbox` でないものは取り直さずに 200。
- 形が不正なら 400。Apple の API の一時的な失敗と、取引が見つからないときは 503 を返して再送させる。
- `CONSUMPTION_REQUEST` には答えない。

## 返金と返金の取り消し (アプリ内課金)

- 通知の種類によらず、取り直した取引と今の行の状態から1つだけ行う (`ocr_quota::apply_app_store_purchase`)。同じ状態なら何もしないので、再送・重複・順序の前後で結果が変わらない。
  - 行が無く、有効: 枠を足す。アプリから届かなかった購入もこれで足される。
  - 行が無く、取り消し済み (`revocationDate` がある): 残り 0 で `revoked_milli_yen` に付与した量を入れた行を作る。後から付与が届いても、行があるので足さない。
  - 有効な行で、取り消し済み: 残りを `revoked_milli_yen` に移して 0 にする。
  - 取り消した行で、取り消しが外れた (`REFUND_REVERSED`): `revoked_milli_yen` を残りに戻す。
- 行がある取引は、取り直した取引の `signedDate` が行の `apple_signed_date` より新しいときだけ変える。取り直してから書くまでに別の経路が新しい状態を書くことがあり、古い状態で上書きしないため。
- 足すのと消すのは同じ `BEGIN IMMEDIATE` の中で行う。
- 持ち主のアカウントが消えていれば、残した行の取り消しの印だけを合わせる (`sync_orphaned_app_store_purchase`)。

## Sandbox

- App Review と TestFlight の購入は Sandbox の取引として本番のサーバーに届く。本番でも Sandbox の取引で枠を足す。
- 環境は `ocr_quota_grants.apple_environment` に残し、使った額の記録では `sandbox` として売上と分ける。
