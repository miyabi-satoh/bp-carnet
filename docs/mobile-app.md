# モバイルアプリ (iPhone・iPad)

ウェブ版の SPA を Capacitor で包んだ iOS アプリ。
ウェブ版の仕組みは変えず、アプリ用の受け付け方をサーバーの横に足している。
ビルドとシミュレータでの動かし方は DEVELOPMENT.md。

## 置き場所

- `mobile/ios`: Xcode のプロジェクト。`mobile/ios/App/App/public` (同梱する画面) は `cap sync` が `frontend/build` から写す生成物。
- `frontend/capacitor.config.ts`: Capacitor の設定。`ios.path` で `mobile/ios` を指す。
  - 設定とプラグインの依存は `frontend/package.json` の側に置く。Capacitor は `cap sync` を走らせる側の `package.json` からプラグインを探すため。
- `mobile/plugins/native-login`: Google・LINE・Apple のログインを呼ぶ自前のプラグイン (Swift)。JS 側は `frontend/src/lib/native-login.ts`。
- `mobile/plugins/app-store-purchase`: アプリ内課金 (StoreKit 2)。JS 側は `frontend/src/lib/app-store-purchase.ts` (→ docs/payments.md)。
- `frontend/src/lib/native-app.ts`: アプリの中かどうかの判定と、ネイティブの機能の呼び分け。
- アプリの ID は `com.amiiby.bpcarnet`。対応は iOS 16.4 以降。
- iPhone は縦向きだけにする (`Info.plist` の `UISupportedInterfaceOrientations`)。横向きは高さが足りず、シートやフォームが画面に収まらないため。iPad は全部の向きに対応する (`~ipad`)。

## 画面は同梱する

- 画面はアプリに同梱し、サーバーから読み込まない。通信が切れても画面は出る。
- コードは `frontend/` をウェブと共用し、ビルドだけを変える (`just app-sync`)。
- アプリの中かどうかは `Capacitor.isNativePlatform()` で見る (`isNativeApp`)。アプリの中では:
  - API の呼び先を公開 URL にする (`API_ORIGIN`)。`VITE_APP_API_ORIGIN` で開発用のサーバーに差し替えられる (`just app-sync-dev`)。
  - Service Worker を登録しない。
- 画面のオリジンは `capacitor://localhost` になる。サーバーから見ると別のサイトで、これが下の認証の作りを決めている。

## 認証: Cookie の代わりにトークン

アプリの画面は別のサイトなので、`SameSite=Strict` のセッション Cookie は送られず、同一オリジンのチェック (docs/architecture.md「CSRF」) でも弾かれる。
そのためアプリはトークンで認証する。

- ログインが済むと、サーバーはトークンを返す (`src/app_token.rs`)。
  - アプリはキーチェーンにしまい (`whenUnlockedThisDeviceOnly`。バックアップから別の端末へは移らない)、毎回 `Authorization: Bearer` で送る (`frontend/src/lib/api/client.ts`)。
  - サーバーは SHA-256 のハッシュだけを `app_tokens` に持つ (ユーザー・`session_generation`・作成日時・最終利用日時)。
- 失効はセッションとそろえる。
  - 最終利用から `[session] expiry_days` 日 (既定 30)。
  - パスワードの変更で `users.session_generation` がずれたとき。変更した端末のトークンだけは新しい世代に載せ替える。
  - 凍結・アカウントの削除 (`ON DELETE CASCADE`)・ログアウト (`POST /api/v1/app/auth/logout`)。
- `AuthUser` (`src/auth.rs`) は、`Authorization: Bearer` があればトークンで、無ければセッションで判定する。両方あるときはトークンだけを見る。

### CORS と同一オリジンのチェック

- CORS は `capacitor://localhost` だけを許し、`Authorization`・`Content-Type`・`X-App-Version` を通す。Cookie は許さない (credentials 無し)。
- preflight (OPTIONS) には、同一オリジンのチェックより外側の CORS の層で答える。
- `Origin: capacitor://localhost` のリクエストは、同一オリジンのチェックから外す。
  - ログインの入口はまだトークンを持たないので、トークンの有無ではなく出どころで見る。
  - アプリの WebView はサーバーの Cookie を送らず、ログインの証しはアプリが自分で付けるトークンだけなので、なりすましの操作に使われる Cookie が無い。
- `Origin` は偽れるので、出どころの判定 (`app_token::is_from_app`) は表示を変えることと版の確認にだけ使う。

## ログインの入口

`/api/v1/app/auth/...` (`src/api/app_auth.rs`)。
名寄せ・同意の記録・凍結や削除予約の扱いは、ウェブのログインと同じ処理を通し、最後にセッションの代わりにトークンを返す。
各方式の詳細は docs/authentication.md。

- ID とパスワード (`/app/auth/login`): ウェブと同じ照合とレートリミット。
- Google (`/app/auth/google`): Google Sign-In を offline モードで使い、サーバー用の認可コード (`serverAuthCode`) を受け取る。
  - 認可コードはウェブ用のクライアントに宛てて発行される。サーバーはウェブ用のクライアント ID・シークレットで引き換え、ウェブと同じく ID トークンで本人を確かめる。
  - アプリには iOS 用のクライアント ID と、宛先のウェブ用のクライアント ID を埋め込む (どちらも秘密ではない)。
  - ID トークンをアプリから受け取って確かめる形にしないのは、手元で署名を確かめる必要が出るため。認可コードは使い捨てで、引き換えにシークレットが要るので `nonce` も要らない。
- LINE (`/app/auth/line`): LINE SDK の ID トークンを、ウェブと同じ LINE の検証エンドポイントで確かめる。
  - SDK はリフレッシュトークンを渡さないので、一緒に受け取るアクセストークンを、退会や連携の解除のときの連携の取り消しに使う。
  - 保存する前に、アクセストークンの持ち主 (`/v2/profile`) が ID トークンの `sub` と同じか確かめる。別の人のものを保存すると、その人の連携を取り消してしまうため。
- Apple (`/app/auth/apple`): iOS 標準の Sign in with Apple で得た認可コードを、`client_id` をアプリの Bundle ID にして引き換える (`redirect_uri` は付けない)。
  - 名前は最初の承認のときだけ Apple が渡すので、アプリが名・姓を一緒に送る。
  - アプリに `com.apple.developer.applesignin` の entitlement を付けている。ボタンは `/auth/providers` の `appleAppEnabled` で出す (ウェブの `appleEnabled` とは要る設定が違う)。
- `nonce` (LINE・Apple): ID トークンの使い回しを防ぐ。
  - ログインを始めるたびにアプリが `POST /app/auth/nonce` で受け取り、SDK に渡す。サーバーは ID トークンの `nonce` と照らし、一度使ったら消す (`src/app_login_nonce.rs`。10分で切れ、プロセスのメモリにだけ持つ)。
  - Apple にはハッシュにせずそのまま渡す。サーバーが発行した値そのものと照らすため。

### メールのリンクを使う手続き

- サインアップ・パスワードの再設定・メールアドレスの変更は、メールのリンクがブラウザで開くので、ブラウザで済ませてからアプリでログインしてもらう (Universal Links は作らない)。
- アプリからの申し込みなら、メールのリンクに `app=1` を付ける (`src/api/email_link.rs`)。リンクの先の画面は、済んだらウェブのホーム画面やログイン画面へ進めず、アプリに戻るよう案内する。

## ネイティブの機能に替えるもの

WebView のままでは動かないものを、`frontend/src/lib/native-app.ts` で呼び分ける。

- 印刷: `window.print()` は何もしないので、iOS の印刷を呼ぶ `@capgo/capacitor-printer` の `printWebView` に替える。
- CSV の書き出し: ダウンロード (`a[download]`) は何もしないので、キャッシュのディレクトリにファイルを書き、共有シートで渡す。渡し終えたらファイルを消す。
- Google・LINE のログイン: ウェブのリダイレクトだとアプリの外の Safari が開いて戻れないので、上のネイティブの入口を使う。
- ステータスバー: 既定は端末の外観に従うので、画面のライト・ダークに合わせて `SystemBars.setStyle` で文字色を変える。
- 言語: `Info.plist` の `CFBundleDevelopmentRegion`・`CFBundleLocalizations` を `ja` だけにする。iOS はアプリが対応する言語から OS の部品 (写真の選択・共有シート・日付のピッカーなど) の言語を選ぶため。
- OCR 枠の買い足しは、アプリではアプリ内課金で売る。サーバーが受け付けないとき (`/ocr/status` の `appStoreTopupAvailable` が `false`) は導線を出さない (→ docs/payments.md)。
- ログイン画面の紹介ページ (`[server] intro_url`) へのリンクは、アプリには出さない。紹介ページはウェブでの買い足しを案内しており、アプリ内課金の外へ導くことになるため (App Review Guidelines 3.1.1)。

## 通信できないとき

- 画面は同梱なので出る。ログインの確認 (`/auth/me`) が届かなければ、ページの本文の代わりにつながらない旨と再試行のボタンを出す (`frontend/src/routes/+layout.ts` の `loadFailure`)。
- サーバーが 5xx を返したときも同じ形で出し、ログイン画面へは送らない。セッションやトークンは有効なままかもしれないため。ウェブも同じ。

## 古い版のアプリ

利用者が更新するまで古い版が残るので、API を変えて合わなくなった版には更新を促す。

- アプリは API の呼び出しに自分の版 (`CFBundleShortVersionString`) を `X-App-Version` で付ける。
- サーバーは、アプリからの `/api/` の呼び出しで、版が `[mobile_app] min_version` より古い・版が無いものを 426 (`app_update_required`) で断る (`src/app_version.rs`)。設定が無ければ見ない。ウェブからの呼び出しは見ない。
- アプリは 426 を受けると、どの画面の代わりにも更新を促す画面を出す (`frontend/src/lib/app-update.svelte.ts`)。
- 断りの応答にもアプリが読めるよう、この層は CORS の層の内側に置いている。
