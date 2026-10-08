# 第三者のソフトウェア

配る物に入っているほかの人のソフトウェア (依存の部品) と、そのライセンスの条文を出すページ `/licenses`。

## どこに出すか

- 設定の「利用規約」「プライバシーポリシー」の並びから開く。URL を直に開けば、サインインしなくても見られる。
- 名前は「第三者のソフトウェア」。
- 節は2つ。
  - 「画面」: ブラウザと iPhone アプリに配る画面 (npm の部品)。
  - 「iPhone アプリ」: iPhone アプリに入るネイティブの部品。アプリで開いたときだけ出す (ブラウザには配っていないため)。
- iPhone アプリでは、アプリに同梱した一覧を読む。ネットにつながらなくても出る。
- サーバー (Rust) の部品は出さない。利用者に配らないため。

## 一覧の形

- **1パッケージ1行**で名前の順に並べ、行には名前・版・ライセンスの式 (例: `Apache-2.0 AND MIT`) を出す。同じ名前の違う版は1行にまとめ、版を並べる。
- 行を開くと、ソースの置き場所と、そのパッケージが同梱している条文を全部出す (著作権表示を含む)。
  - ソースの置き場所はリンクにし、別のタブで開く。MPL-2.0 の部品 (`@capgo/capacitor-printer`) があり、受け取る人にソースの入手先を知らせる必要があるため。
- 一覧を読めなかった節は「一覧を読み込めませんでした。」と出し、もう片方は出す。

## どう作るか

一覧は静的な JSON (`{ texts, packages }`)。抽出した結果を、共通の `frontend/scripts/license-display.ts` が画面に出す形に整える (1パッケージ1件にまとめる、同じ条文を1つにする)。本文の無いもの、著作権者が空欄のひな形が残っていたら、書き出さずに止める。

- **画面の分**は Vite のプラグイン (`frontend/scripts/third-party-licenses.ts`) が、ビルドのたびに `build/third-party-licenses/npm.json` に書き出す。リポジトリには入れない。
  - `package.json` の依存ではなく、実際に配る JavaScript・CSS・フォントに入ったモジュールから集める (lint やテストの道具を混ぜないため)。
  - Service Worker の実行部分 (workbox) は @vite-pwa/sveltekit が別に作るので、プラグインの中で名前を挙げて足す。
  - 開発サーバーには無いので、`just dev` では「画面」が読み込めない表示になる。
- **iPhone アプリの分**は `just licenses` (`frontend/scripts/generate-ios-licenses.ts`) が作り、`frontend/static/third-party-licenses/ios.json` としてリポジトリに入れる。
  - Swift Package Manager の部品は、`Package.resolved` の固定したコミットのライセンスのファイルを、上流のリポジトリから取る (取ったリポジトリは `data/license-cache/` に置く)。ライセンスの種類は本文から見分ける。
  - Capacitor のプラグインは、`CapApp-SPM/Package.swift` が指す node_modules の中のパッケージから取る。
  - 依存を変えたら `just licenses` で作り直してコミットする。`just ci` の `licenses-check` が、一覧の部品と版が今の依存と合っているかを見る (ネットを見ないので、作り直しまではしない)。
- どちらも、配ってよいライセンスの一覧 (`license-display.ts` の `ACCEPTED`) に無いものが入ると、作るのを止める。中身を確かめてから一覧に足す。
- サーバーの部品は表示しないが、`licenses-check` が AGPL・SSPL (ネットワーク越しに使わせるだけで条件が掛かるもの) が入っていないかを見る (`frontend/scripts/check-server-licenses.ts`)。
