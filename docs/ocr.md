# 写真の読み取り (OCR)

血圧計の液晶か手書きの記録を写した写真を Gemini に送り、血圧の値を読み取る仕組み。
画面の流れは扱わない。
課金で枠を買い足す仕組みは docs/payments.md。

## 置き場所

- `src/ocr.rs`: Gemini の呼び出し・プロンプト・応答の解釈
- `src/api/ocr.rs`: `POST /api/v1/ocr`・`GET /api/v1/ocr/status`・`POST|DELETE /api/v1/ocr/consent`
- `src/ocr_usage.rs`: 枠の並び (`quotas`)・残りの判定・使った額の引き落とし・同時に1件の制限
- `src/ocr_consent.rs`: 写真を送ることへの同意
- `src/ocr_dump.rs`: デバッグ用に Gemini とのやりとりを残す・使い回す
- `frontend/src/lib/ocr.ts`・`ocr-image.ts`: 送る前の縮小・信頼度の段階・手書きの行の扱い

## 有効化

- 環境変数 `GEMINI_API_KEY` があるときだけ有効。
  - 無ければ `POST /ocr` は 503 (`ocr_disabled`)、`GET /ocr/status` は `enabled: false` を返し、画面は機能を隠す。
- モデルは既定 `gemini-3.8-flash`。環境変数 `GEMINI_MODEL` で差し替えられる。

## Gemini への送り方

- `generateContent` (v1beta) に、写真 (`inline_data`、base64) とプロンプト `PROMPT_AUTO` を1回で送る。
- 写真の種類はモデルに判定させる。単発・複数行でエンドポイントやプロンプトを分けない。
- 構造化出力を使う: `responseMimeType: application/json` と `responseSchema` (`ocr_schema`)、`temperature: 0`。
- 思考の量は `thinkingConfig.thinkingLevel: low`。応答までの時間の大半を思考が占めるため (選んだ理由は `src/ocr.rs` の `DEFAULT_MODEL`・`THINKING_LEVEL`)。
  - モデルを差し替えるときは、思考の量との組で読み比べてから決める。
- `maxOutputTokens` は 32768。
  - 思考 (thinking) のトークンもこの上限に入り、足りないと JSON が途中で切れるため大きめにしている。
- タイムアウトは 90 秒。

## 返す形 (`OcrResult`)

- `kind`:
  - `monitor_bp`: 液晶に血圧値が出ている。`reading` に1件。
  - `monitor_none`: 液晶だが血圧値以外 (日時など) を表示中。
  - `memo`: 手書きの記録。`readings` に行ごと。各行に `measuredOn` (`MM-DD`)・`time` (`HH:MM`、無ければ空)・`period` (朝・夜)・`memo` (添え書き)・`note` (モデルの補足) が付く。
  - `unknown`: どれとも判定できない。
- 値の欠落や型崩れでは失敗させない。
  - `systolic`・`diastolic`・`pulse` は文字列や巨大な数も受け、読めなければ 0 にする (`deserialize_flex_int`)。
  - 項目が丸ごと無い行も 0 (信頼度は 0.0) として残す。
  - 脈拍の 0 は「読み取れなかった」。
- `usage` (入力・出力のトークン数) を返すが、画面には出さない。

## 信頼度と値域の検査

- 各行の `confidence` (0.0〜1.0) はモデルの自己申告。
- 画面での段階 (`frontend/src/lib/ocr.ts`):
  - 0.9 以上が高、0.75 以上が中、それ未満が低。
  - 手書きの行は 0.75 未満を「要確認」にし、最初から取り込みのチェックを外す。
- 単発 (`reading`) だけ、サーバーで値域を検査する (`reading_passes_validation`)。
  - 記録の登録と同じ `validation::validate_bp_values` を使い、値域外や「収縮期 > 拡張期」が崩れた値は `reading` を `null` にする。
  - `kind` は `monitor_bp` のまま残るので、画面は `kind === 'monitor_bp' && reading` で読めたかを判定する。
- 手書きの行 (`readings`) は検査で落とさない。
  - 黙って落とすと欠けたことに気づけないため、利用者が行ごとに確かめて直す前提にしている。

## 画像の扱い

- 送る前に端末で縮小する (`resizeImage`): 長辺 1600px、JPEG 品質 0.85。
  - Exif の回転はピクセルに焼き込む。
- サーバーは jpeg・png・webp だけを受け付け (422)、10MB を超えるものは 413 (`ocr_image_too_large`)。
- 写真はメモリで扱うだけで、保存しない。
  - 例外は `[ocr] dump_dir` を設定したときだけ (下の「デバッグ用の設定」)。
  - ログにも写真の内容 (読み取った文字) を出さない。解釈に失敗したときも長さだけを出す。

## 同時に1件

- 同じユーザーの読み取りは1件ずつ (`ocr_usage::InFlight`)。2件目は 409 (`ocr_in_progress`)。
- 枠は「呼ぶ前に残りがあるか」だけを見て、呼んだ後で引くため、同時に呼ばれた数だけ残りを超えて使える。1件ずつにして、超える分を最後の1回分に抑えている。
- 読み取り中のユーザーはプロセスのメモリに持つ。サーバーを1台で動かす前提。

## 同意

- 写真を Gemini に送る前に、利用者の同意が要る。`users.ocr_consented_at` にアカウントごとに持つ。
- `POST /ocr` は写真を受け取る前に同意を確かめ、無ければ 403 (`ocr_consent_required`)。
- `POST /ocr/consent` で記録 (同意済みなら最初の時刻を残す)、`DELETE /ocr/consent` で取り消す。

## 読み取りの順序 (`POST /ocr`)

1. 有効か (503)・同意しているか (403)
2. 写真の形式と大きさ (400・413・422)
3. `[ocr] reuse_dumps` で使い回せる応答があれば、ここで返す (枠は見ず、額も足さない)
4. 同時に1件 (409)・枠が残っているか (429 `ocr_budget_exhausted`)
5. Gemini を呼び、使った額を引く。失敗は 502

## 使った額の数え方

- 1回の額 = 入力トークン × 入力単価 + 出力トークン × 出力単価 を円に直したもの (`OcrPricing::cost_milli_yen`)。
  - 出力には思考のトークン (`thoughtsTokenCount`) を足す。どちらも出力として課金されるため。
  - 単位は 1/1000 円で、端数は切り上げる。
- 応答が届いた回は、解釈に失敗しても額を引く (`OcrError::ParseResponse { usage }`)。毎回失敗する写真で費用だけがかかり続けないため。
- 通信自体が失敗した回は引かない。
- 引けなかったとき (DB の失敗) は、警告のログを出して読み取りの結果はそのまま返す。

## 枠と上限 (`[ocr]`)

- 無料の枠は、ユーザーごとの累計の金額の上限。
  - 環境変数 `OCR_FREE_BUDGET_YEN` (円、無ければ `[ocr] free_budget_yen`) が既定。負値は無制限。
  - `users.ocr_budget_yen` があればそちらを使う (`NULL` は設定の既定、負値は無制限、0 は使えない)。管理画面から変えられる。
  - 使った累計は `users.ocr_spent_milli_yen`。
- 買い足した枠は購入ごとの行 (`ocr_quota_grants`) で持つ。
- `ocr_usage::quotas` が、枠を使う順に並べる。残りの判定・引き落とし・`GET /ocr/status` の `quotas` はすべてこの並びに従う。
  - 買い足した枠を買った順に先に使い、無料の枠は最後。
  - 無料の枠が無制限なら `None` を返し、買い足した枠にも手を付けない (全額を無料の累計に足す)。
- 使った額は `ocr_usage_records` に、減らした枠ごとに1行ずつ残す (1回の読み取りが複数の枠にまたがれば分ける)。
- `GET /ocr/status` の `quotas` の各枠は残りを割合 (`remainingPercent`、残りが少しでもあれば切り上げて 1 以上) で返し、金額は返さない。

## 単価

- 入力・出力の 100 万トークンあたりのドルと、1ドルの円換算。環境変数 `OCR_INPUT_USD_PER_MILLION_TOKENS`・`OCR_OUTPUT_USD_PER_MILLION_TOKENS`・`OCR_JPY_PER_USD` (無ければ `[ocr.pricing]`) で与える。
- 無料枠と単価はコードに既定値を持たない。`GEMINI_API_KEY` があるのにどれかが無ければ、起動しない (`OcrConfig::costs`)。Gemini の価格が変わったら環境変数で追随する (DEVELOPMENT.md「写真の読み取りの無料枠・単価を変える」)。

## デバッグ用の設定

- `[ocr] dump_dir`: 置いたときだけ、Gemini とのやりとりを呼び出しごとにディレクトリへ残す。
  - 中身は写真 (`image-0.jpg` など)・`request.json`・`response.json`・`meta.json`。相対パスはデータディレクトリ基準。
  - 解釈に失敗した応答も残る (ステータスや本文を確かめる前に書く)。
  - 写真と血圧の値がそのまま残るため、既定では残さない。OCR が有効で `dump_dir` があると、起動のたびに警告を出す。
- `[ocr] reuse_dumps` (開発用): `dump_dir` に、同じモデル・同じ送信内容 (写真・プロンプト・生成の設定) で成功したやりとりがあれば、Gemini を呼ばずにその応答を返す。
  - 画面の確認で料金と枠を使わないため。プロンプトを変えれば送信内容が変わるので、古い応答は使われない。
