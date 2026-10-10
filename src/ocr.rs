//! 血圧計の液晶、または手書きの血圧記録メモを撮影した写真から、Gemini の
//! `generateContent` エンドポイントで血圧値 (収縮期・拡張期・脈拍) を読み取る OCR 機能。
//!
//! スコープ (現時点): 血圧計液晶の単発読み取り (`monitor_bp`)・手書きメモの複数行読み取り
//! (`memo`)。推定コスト表示の UI 反映は含まない
//! (`OcrUsage` は既に返しているため、追加時はこのモジュールへの追記で対応できる)。

use std::path::PathBuf;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use utoipa::ToSchema;

use crate::config::non_empty_env;
use crate::ocr_dump;
use crate::upstream;
use crate::validation::validate_bp_values;

/// Gemini の API キーの環境変数。機微情報のため config.toml には置かない。
pub const API_KEY_ENV: &str = "GEMINI_API_KEY";
/// 使う Gemini のモデル名を差し替える環境変数 (任意)。
pub const MODEL_ENV: &str = "GEMINI_MODEL";
/// `GEMINI_MODEL` が無いときに使うモデル。
///
/// ADR: モデルと思考の量 ([`THINKING_LEVEL`]) は組で選ぶ。手元の写真15枚 (液晶・27行の手書きメモなど) を
/// 7通りの組で読み比べ、正しさを保ったまま速い組にした。前の `gemini-3.6-flash` (思考は既定の medium) は
/// メモ 約24秒・液晶 約10秒、この組は メモ 約9秒・液晶 約4秒で、正しく読めた行の割合は変わらなかった。
const DEFAULT_MODEL: &str = "gemini-3.8-flash";
/// `generateContent` の `thinkingConfig.thinkingLevel`。応答までの時間の大半は思考が占める。
///
/// ADR: `low` にする。`gemini-3.6-flash` で `low`・`minimal` に下げると、2列に書いたメモの読む順が乱れて
/// 日付の付け間違いが増えたが、`gemini-3.8-flash` の `low` では増えなかった。`gemini-3.5-flash-lite` は
/// さらに速いが、行を読み落とした写真があった。
const THINKING_LEVEL: &str = "low";

/// Gemini の `generateContent` エンドポイント (v1beta)。モデル名だけ差し替える。
///
/// ADR: Gemini API には `generateContent` の後継として Interactions API がある
/// (https://ai.google.dev/gemini-api/docs/interactions)。`generateContent` は
/// legacy 扱いだが "remains fully supported" で停止時期は未告知のため、今は
/// 据え置く。移行を検討する際、Interactions API は既定でリクエスト/レスポンスを
/// サーバー側に保存する (無料枠1日・有料枠55日) 点に注意する。本機能が扱うのは
/// 血圧計・手書きメモの画像という健康データのため、`store=false` の可否を必ず確認する。
const GEMINI_ENDPOINT: &str = "https://generativelanguage.googleapis.com/v1beta/models";

/// ADR: `generateContent` の `maxOutputTokens`。思考 (thinking) のトークンもこの上限に含まれ
/// (https://ai.google.dev/gemini-api/docs/thinking)、足りないと JSON が途中で切れて解析に失敗する。
/// 27行の手書きメモで思考が 2千〜6.5千トークンと揺れ、出力 (1.2千〜2.2千) と合わせて 8192 に
/// 届いた回があったため、実測の最大の4倍ほどにする。上限を上げても課金は実際に生成した分だけ。
const MAX_OUTPUT_TOKENS: u32 = 32_768;

/// 血圧計液晶の OCR を Gemini に依頼するサービス。`api_key` が `None` なら無効化されており、
/// ハンドラ側は 503 を返す (フロントエンドはこれを見て機能自体を隠す)。
pub struct OcrService {
    api_key: Option<String>,
    model: String,
    client: reqwest::Client,
    /// `[ocr] dump_dir`。`Some` なら Gemini とのやりとりをこの下に残す (→ [`crate::ocr_dump`])。
    dump_dir: Option<PathBuf>,
    /// `[ocr] reuse_dumps`。`dump_dir` に同じ送信内容の応答が残っていれば、Gemini を呼ばずに使い回す (開発用)。
    reuse_dumps: bool,
}

// `api_key` をログに出さないよう手書きする (`SessionConfig` と同じ理由)。
impl std::fmt::Debug for OcrService {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OcrService")
            .field("api_key", &self.api_key.as_ref().map(|_| "<redacted>"))
            .field("model", &self.model)
            .field("dump_dir", &self.dump_dir)
            .field("reuse_dumps", &self.reuse_dumps)
            .finish()
    }
}

impl OcrService {
    fn new(api_key: Option<String>, model: String, client: reqwest::Client) -> Self {
        Self {
            api_key,
            model,
            client,
            dump_dir: None,
            reuse_dumps: false,
        }
    }

    /// `GEMINI_API_KEY` (空文字列は未設定扱い) と、任意で `GEMINI_MODEL` から構築する。
    pub fn from_env() -> Self {
        Self::new(
            non_empty_env(API_KEY_ENV),
            non_empty_env(MODEL_ENV).unwrap_or_else(|| DEFAULT_MODEL.to_string()),
            // 手書きメモは複数行分のレスポンスを一度に生成させるため、単発読み取りより
            // Gemini 側の生成に時間がかかりうる。PoC の値 (90s) を踏襲して余裕を持たせる。
            reqwest::Client::builder()
                .timeout(Duration::from_secs(90))
                .build()
                .expect("failed to build reqwest client"),
        )
    }

    /// Gemini とのやりとりを `dir` の下に残すようにする (`None` なら残さない)。
    pub fn with_dump_dir(mut self, dir: Option<PathBuf>) -> Self {
        self.dump_dir = dir;
        self
    }

    /// `dump_dir` に残したやりとりを使い回すようにする (→ [`Self::reuse_dumped`])。
    pub fn with_reuse_dumps(mut self, reuse: bool) -> Self {
        self.reuse_dumps = reuse;
        self
    }

    /// 常に無効化されたサービス。統合テストのように、実行環境の `GEMINI_API_KEY` に
    /// 依存せず「OCR 無効時」の挙動を明示的に検証したい場合に使う。
    pub fn disabled() -> Self {
        Self::new(None, DEFAULT_MODEL.to_string(), reqwest::Client::new())
    }

    /// 任意の API キーでサービスを構築する。統合テストで「OCR 有効時」の挙動
    /// (画像サイズ・MIME タイプの事前検証など、Gemini 呼び出し前に弾かれるもの) を
    /// 検証する際に、ダミーの値で `enabled() == true` を作るために使う。
    pub fn with_api_key(api_key: impl Into<String>) -> Self {
        Self::new(
            Some(api_key.into()),
            DEFAULT_MODEL.to_string(),
            reqwest::Client::new(),
        )
    }

    pub fn enabled(&self) -> bool {
        self.api_key.is_some()
    }

    /// 開発用 (`[ocr] reuse_dumps`)。同じモデル・同じ送信内容で成功したやりとりが `dump_dir` に残っていれば、
    /// Gemini を呼ばずにその応答から読み取り結果を返す。画面の見た目を確かめるたびに、読み取りの回数と料金を
    /// 使わないため (docs/ocr.md)。無ければ `None` (呼び出し側が Gemini を呼ぶ)。
    pub async fn reuse_dumped(&self, image_bytes: &[u8], mime_type: &str) -> Option<OcrResult> {
        if !self.reuse_dumps {
            return None;
        }
        let dir = self.dump_dir.clone()?;
        let request = serde_json::to_value(request_body(image_bytes, mime_type)).ok()?;
        let body = ocr_dump::find_response(dir, self.model.clone(), request).await?;
        match parse_gemini_response(&body) {
            Ok(result) => {
                tracing::info!("残したやりとりの応答を使い回しました (Gemini は呼んでいません)");
                Some(result)
            }
            Err(err) => {
                tracing::warn!(error = %crate::error_chain_line(&err), "残したやりとりの応答を解釈できませんでした");
                None
            }
        }
    }

    /// 画像を Gemini に送り、血圧計液晶の読み取り結果を返す。
    pub async fn extract(
        &self,
        image_bytes: &[u8],
        mime_type: &str,
    ) -> Result<OcrResult, OcrError> {
        let api_key = self.api_key.as_deref().ok_or(OcrError::Disabled)?;
        let request_body = request_body(image_bytes, mime_type);
        let raw_body = self.post_generate_content(api_key, &request_body).await?;
        parse_gemini_response(&raw_body)
    }

    /// Gemini `generateContent` を呼び、成功時のレスポンス本文を返す。
    async fn post_generate_content(
        &self,
        api_key: &str,
        request_body: &GeminiRequest,
    ) -> Result<String, OcrError> {
        let url = format!("{GEMINI_ENDPOINT}/{}:generateContent", self.model);
        let started = Instant::now();
        let received = match self
            .client
            .post(&url)
            .header("x-goog-api-key", api_key)
            .json(request_body)
            .send()
            .await
        {
            Ok(response) => upstream::response_body(response)
                .await
                .map_err(OcrError::from),
            Err(err) => Err(OcrError::from(err)),
        };

        // 解釈に失敗した応答こそ見たいので、ステータスや本文を確かめる前に残す。
        if let Some(dir) = &self.dump_dir {
            let exchange = ocr_dump::Exchange {
                model: self.model.clone(),
                request: serde_json::to_value(request_body).unwrap_or(Value::Null),
                outcome: match &received {
                    Ok((status, body)) => Ok((status.as_u16(), body.clone())),
                    Err(err) => Err(err.to_string()),
                },
                elapsed: started.elapsed(),
            };
            ocr_dump::save(dir.clone(), exchange).await;
        }

        let (status, raw_body) = received?;
        if !status.is_success() {
            tracing::warn!(status = %status, "Gemini がエラーを返しました");
            return Err(OcrError::UpstreamStatus(status.as_u16()));
        }
        Ok(raw_body)
    }
}

/// Gemini に送る本文。読み取り (`extract`) と、残したやりとりを探すとき (`reuse_dumped`) で同じものを使う。
fn request_body(image_bytes: &[u8], mime_type: &str) -> GeminiRequest {
    GeminiRequest {
        contents: vec![GeminiContent {
            parts: vec![
                GeminiPart {
                    text: None,
                    inline_data: Some(GeminiInlineData {
                        mime_type: mime_type.to_string(),
                        data: base64::Engine::encode(
                            &base64::engine::general_purpose::STANDARD,
                            image_bytes,
                        ),
                    }),
                },
                GeminiPart {
                    text: Some(PROMPT_AUTO.to_string()),
                    inline_data: None,
                },
            ],
        }],
        generation_config: GeminiGenerationConfig {
            response_mime_type: "application/json".to_string(),
            response_schema: ocr_schema(),
            temperature: 0.0,
            max_output_tokens: MAX_OUTPUT_TOKENS,
            thinking_config: GeminiThinkingConfig {
                thinking_level: THINKING_LEVEL,
            },
        },
    }
}

#[derive(Debug, thiserror::Error)]
pub enum OcrError {
    #[error("OCR is not configured (GEMINI_API_KEY missing)")]
    Disabled,
    #[error("failed to call Gemini")]
    Request(#[source] reqwest::Error),
    #[error("Gemini returned status {0}")]
    UpstreamStatus(u16),
    /// 応答の解釈に失敗した。`usage` は、応答自体は届いて (課金されて) いたときの使用量。
    /// 届いた応答の形が壊れていて取り出せなかったときは `None`。累計の金額に足すために持つ。
    #[error("failed to parse Gemini response")]
    ParseResponse { usage: Option<OcrUsage> },
}

impl From<reqwest::Error> for OcrError {
    /// `without_url()`: エラーメッセージに URL を含めない。API キーはヘッダーで送るため
    /// URL 自体に機微情報は無いが、念のための多層防御。
    fn from(err: reqwest::Error) -> Self {
        Self::Request(err.without_url())
    }
}

impl From<upstream::Error> for OcrError {
    fn from(err: upstream::Error) -> Self {
        match err {
            upstream::Error::Request(err) => Self::from(err),
            upstream::Error::UpstreamStatus(status) => Self::UpstreamStatus(status),
            upstream::Error::ParseResponse => Self::ParseResponse { usage: None },
        }
    }
}

/// クライアントに返す OCR 結果。
///
/// - `monitor_bp`: 血圧計液晶の単発読み取り。血圧値が読み取れた (`reading` を必ず埋める)
/// - `monitor_none`: 液晶だが血圧値以外 (日時・気温など) が表示されている
/// - `memo`: 手書きの血圧記録メモ (複数行)。行ごとの読み取り結果を `readings` に埋める
/// - `unknown`: 血圧計の液晶・手書きメモのいずれとも判定できなかった
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct OcrResult {
    pub kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub raw_display: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reading: Option<OcrReading>,
    /// `memo` のときの行ごとの読み取り結果。他の `kind` では空配列。
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub readings: Vec<OcrReading>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
    pub usage: OcrUsage,
}

/// 血圧値 1 件分の読み取り結果。単発読み取り (`OcrResult::reading`) と手書きメモの
/// 行ごとの読み取り結果 (`OcrResult::readings`) の両方で共用する。`measured_on`/`time`/`note`
/// は手書きメモでのみ使う (単発読み取りでは常に `None`)。
#[derive(Debug, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct OcrReading {
    /// "MM-DD" 形式。手書きメモでのみ使う。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(example = "09-05")]
    pub measured_on: Option<String>,
    /// "HH:MM" 形式。手書きメモでのみ使う。時刻が読み取れない行は `None` のまま
    /// (でっち上げない)。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(example = "07:30")]
    pub time: Option<String>,
    // `default`: スキーマで必須にしていても、Gemini が特定の行だけこのフィールド自体を
    // 省略しうる。無ければ「読み取れなかった (0)」として扱う (`deserialize_flex_int` が値ありだが解釈不能な場合に
    // 0 へ丸めるのと同じ考え方)。欠落を理由に行1件・レスポンス全体を丸ごと失敗させない。
    // `#[schema(required = true)]`: `serde(default)` を付けると utoipa は「この API の
    // レスポンスでは省略されうる」と解釈して自動的に required から外してしまうが、
    // それは Gemini からの入力 (デコード) にのみ言える話であり、この型を使う自分の
    // API のレスポンス (エンコード、こちらは常に埋めて返す) には当てはまらない。
    // OpenAPI 契約 (延いては生成される TypeScript 型) を崩さないよう明示的に上書きする。
    #[serde(default, deserialize_with = "deserialize_flex_int")]
    #[schema(example = 128, required = true)]
    pub systolic: i64,
    #[serde(default, deserialize_with = "deserialize_flex_int")]
    #[schema(example = 82, required = true)]
    pub diastolic: i64,
    #[serde(default, deserialize_with = "deserialize_flex_int")]
    #[schema(example = 70, required = true)]
    pub pulse: i64,
    /// 0.0-1.0。読み取りの自信度 (フロントエンドで高・中・低の3段階の表示に変換する)。
    /// 省略時は 0.0 (最も低い信頼度) 扱いにする (理由は systolic 等と同じ)。
    #[serde(default)]
    #[schema(example = 0.95, required = true)]
    pub confidence: f64,
    /// この行について Gemini が補足したいこと (判読できない字・曖昧な時刻表現等)。
    /// 手書きメモでのみ使う。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    /// 手帳でその行に添えて書かれた文章 (体調・服薬など)。記録のメモ欄に取り込む。
    /// 手書きメモでのみ使う。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub memo: Option<String>,
    /// 手帳でその行に添えて書かれた朝・夜 (「朝」「起床時」「夜」「就寝前」など)。時刻の無い行に
    /// 仮の時刻を振るときに使う。書かれていない・どちらとも取れない行は `None`。手書きメモでのみ使う。
    #[serde(
        default,
        deserialize_with = "deserialize_period",
        skip_serializing_if = "Option::is_none"
    )]
    pub period: Option<OcrPeriod>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub enum OcrPeriod {
    Morning,
    Evening,
}

/// Gemini には `"none"` も選ばせる (列挙で空文字を避けるため)。`"none"` と想定外の値は `None` にする
/// (補足が読めないことを理由に行を落とさない。`deserialize_flex_int` と同じ考え方)。
fn deserialize_period<'de, D>(deserializer: D) -> Result<Option<OcrPeriod>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    Ok(match Value::deserialize(deserializer)? {
        Value::String(s) if s == "morning" => Some(OcrPeriod::Morning),
        Value::String(s) if s == "evening" => Some(OcrPeriod::Evening),
        _ => None,
    })
}

#[derive(Debug, Clone, Copy, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct OcrUsage {
    pub input_tokens: i64,
    pub output_tokens: i64,
}

/// `systolic`/`diastolic`/`pulse` は Gemini の構造化出力が稀に int からはみ出す値
/// (例: 巨大な指数表記) や文字列で返してくることがあるため、そのまま `i64` として
/// デコードせず一度 `serde_json::Value` で受けてから寛容にパースする。
/// 解釈できない・範囲外の値は (クラッシュさせず) 0 に丸める。
fn deserialize_flex_int<'de, D>(deserializer: D) -> Result<i64, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = Value::deserialize(deserializer)?;
    Ok(match value {
        Value::Number(n) => n
            .as_i64()
            .or_else(|| n.as_f64().map(clamp_to_i64))
            .unwrap_or(0),
        Value::String(s) => s.trim().parse::<i64>().unwrap_or(0),
        _ => 0,
    })
}

fn clamp_to_i64(v: f64) -> i64 {
    if !v.is_finite() || v > i32::MAX as f64 || v < i32::MIN as f64 {
        0
    } else {
        v as i64
    }
}

const PROMPT_AUTO: &str = "あなたは家庭用血圧計の液晶、または手書きの血圧記録メモを読み取る OCR です。写真は回転している場合があり、補正した前提で読み取ってください。

まず何が写っているかを判定してください:
- 血圧計の液晶に血圧値 (収縮期/拡張期/脈拍) が表示されている → kind: \"monitor_bp\"、reading を必ず埋める
- 血圧計の液晶だが血圧値以外 (日時・気温など) を表示中 → kind: \"monitor_none\"、notes に何が写っているか書く
- 日付ごとに複数行の血圧値が並んだ手書きの記録メモ → kind: \"memo\"、readings に行ごとの結果を配列で埋める
- 上記のいずれとも判定できない → kind: \"unknown\"、notes に何が写っているか書く

kind が \"monitor_bp\" のとき:
- rawDisplay に液晶の全数字・記号を必ず書く
- 数値をでっち上げず、不明瞭なら confidence を下げる

kind が \"memo\" のとき、readings の各要素について:
- measuredOn は \"MM-DD\" 形式に正規化する。日付の記載が無い行は直前の行の日付を引き継ぐ
  (メモは日付ごとにまとめて書かれることが多い)
- time は \"HH:MM\" 形式。明示的な時刻表記が無い行 (\"朝\"「起床時」等の曖昧語のみ) は
  空文字列のままにし、でっち上げない
- period には、その行に朝・夜の区別が書かれていれば \"morning\" / \"evening\" を入れる
  (「朝」「起床時」「朝食前」→ morning、「夜」「晩」「就寝前」→ evening。表の列見出しで
  分かれている場合も同じ)。書かれていない・どちらとも取れない行は \"none\"。時刻がある行も埋める。
  period に入れた語は note に書かない。朝・夜のどちらとも取れない曖昧語は note に書く
- 数値の役割 (収縮期・拡張期・脈拍) は位置や値の大小関係から判断する。血圧の定義上
  収縮期は拡張期より必ず大きい。脈拍の記載が無い行は 0 のままにする
- 判読できない値・行は無理に埋めず confidence を下げ、理由を note に書く
- memo には、その行に添えて手帳に書かれた文章 (体調・服薬など) を書かれたとおりに入れる。
  日付・時刻・血圧値・脈拍そのものは含めない。文章が無い行は空文字列にする。
  note (あなたからの補足) とは混ぜない
- 読み取れた行は確信が低くてもすべて readings に含める (勝手に間引かない)";

/// `reading`/`readings` (行1件分) の JSON Schema。単発読み取り・手書きメモ複数行読み取りで共有する。
fn reading_schema() -> Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "measuredOn": { "type": "string" },
            "time": { "type": "string" },
            "systolic": { "type": "integer" },
            "diastolic": { "type": "integer" },
            "pulse": { "type": "integer" },
            "confidence": { "type": "number" },
            "note": { "type": "string" },
            "memo": { "type": "string" },
            "period": { "type": "string", "enum": ["morning", "evening", "none"] }
        },
        // 必須にしないと、Gemini が信頼度や血圧値を省いた行を返すことがある。
        "required": ["measuredOn", "time", "systolic", "diastolic", "pulse", "confidence", "note", "memo", "period"]
    })
}

fn ocr_schema() -> Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "kind": {
                "type": "string",
                "enum": ["monitor_bp", "monitor_none", "memo", "unknown"]
            },
            "rawDisplay": { "type": "string" },
            "reading": reading_schema(),
            "readings": { "type": "array", "items": reading_schema() },
            "notes": { "type": "string" }
        },
        "required": ["kind"]
    })
}

// --- Gemini ワイヤー型 ------------------------------------------------------

#[derive(Debug, Serialize)]
struct GeminiRequest {
    contents: Vec<GeminiContent>,
    #[serde(rename = "generationConfig")]
    generation_config: GeminiGenerationConfig,
}

#[derive(Debug, Serialize)]
struct GeminiContent {
    parts: Vec<GeminiPart>,
}

#[derive(Debug, Serialize)]
struct GeminiPart {
    #[serde(skip_serializing_if = "Option::is_none")]
    text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", rename = "inline_data")]
    inline_data: Option<GeminiInlineData>,
}

#[derive(Debug, Serialize)]
struct GeminiInlineData {
    #[serde(rename = "mime_type")]
    mime_type: String,
    data: String,
}

#[derive(Debug, Serialize)]
struct GeminiGenerationConfig {
    #[serde(rename = "responseMimeType")]
    response_mime_type: String,
    #[serde(rename = "responseSchema")]
    response_schema: Value,
    temperature: f64,
    #[serde(rename = "maxOutputTokens")]
    max_output_tokens: u32,
    #[serde(rename = "thinkingConfig")]
    thinking_config: GeminiThinkingConfig,
}

#[derive(Debug, Serialize)]
struct GeminiThinkingConfig {
    #[serde(rename = "thinkingLevel")]
    thinking_level: &'static str,
}

#[derive(Debug, Default, Deserialize)]
struct GeminiResponse {
    #[serde(default)]
    candidates: Vec<GeminiCandidate>,
    #[serde(default, rename = "usageMetadata")]
    usage_metadata: GeminiUsageMetadata,
}

#[derive(Debug, Deserialize)]
struct GeminiCandidate {
    #[serde(default)]
    content: GeminiResponseContent,
    #[serde(default, rename = "finishReason")]
    finish_reason: String,
}

#[derive(Debug, Default, Deserialize)]
struct GeminiResponseContent {
    #[serde(default)]
    parts: Vec<GeminiResponsePart>,
}

#[derive(Debug, Default, Deserialize)]
struct GeminiResponsePart {
    #[serde(default)]
    text: String,
}

#[derive(Debug, Default, Deserialize)]
struct GeminiUsageMetadata {
    #[serde(default, rename = "promptTokenCount")]
    prompt_token_count: i64,
    #[serde(default, rename = "candidatesTokenCount")]
    candidates_token_count: i64,
    /// 思考 (thinking) のトークン。`candidatesTokenCount` には含まれないが、出力として課金される
    /// (https://ai.google.dev/gemini-api/docs/pricing)。
    #[serde(default, rename = "thoughtsTokenCount")]
    thoughts_token_count: i64,
}

impl GeminiUsageMetadata {
    fn usage(&self) -> OcrUsage {
        OcrUsage {
            input_tokens: self.prompt_token_count,
            output_tokens: self
                .candidates_token_count
                .saturating_add(self.thoughts_token_count),
        }
    }
}

/// Gemini からの生レスポンス (JSON 文字列) をパースする。ハンドラ抜きでテストできるよう
/// 純粋関数として切り出している (異常な数値・空 candidates・MAX_TOKENS 打ち切り等を単体で確認する)。
fn parse_gemini_response(raw: &str) -> Result<OcrResult, OcrError> {
    let response: GeminiResponse =
        serde_json::from_str(raw).map_err(|_| OcrError::ParseResponse {
            // 候補側の型崩れでも、課金された分は usageMetadata だけ取り出して累計に入れる。
            usage: serde_json::from_str::<serde_json::Value>(raw)
                .ok()
                .and_then(|v| v.get("usageMetadata").cloned())
                .and_then(|m| serde_json::from_value::<GeminiUsageMetadata>(m).ok())
                .map(|m| m.usage()),
        })?;
    // ここから先の失敗は、応答が届いて課金された後のもの (MAX_TOKENS の打ち切りなど)。
    let parse_error = || OcrError::ParseResponse {
        usage: Some(response.usage_metadata.usage()),
    };

    let candidate = response.candidates.first().ok_or_else(parse_error)?;
    let inner_text = candidate
        .content
        .parts
        .first()
        .map(|p| p.text.as_str())
        .ok_or_else(parse_error)?;

    if candidate.finish_reason == "MAX_TOKENS" {
        tracing::warn!("Gemini のレスポンスが MAX_TOKENS で打ち切られました");
    }

    let payload: OcrPayload = serde_json::from_str(inner_text).map_err(|err| {
        // `inner_text` は撮影した写真の内容 (rawDisplay/notes) を含みうるため、パース
        // 失敗時であってもログに内容そのものを出さない (長さのみ記録する)。
        tracing::error!(
            error = %crate::error_chain_line(&err),
            inner_len = inner_text.len(),
            "Gemini の構造化出力の JSON 解析に失敗しました"
        );
        parse_error()
    })?;

    let kind = if payload.kind.is_empty() {
        "unknown".to_string()
    } else {
        payload.kind
    };
    // reading はモデルが monitor_bp と言いつつ壊れた値 (空・片方だけ・値域外) を
    // 返すことがあるため、`crate::validation` (records.rs の登録 API と共有) の値域に
    // 収まっており、かつ収縮期 > 拡張期 (血圧の定義上必ず成り立つ関係) の場合のみ
    // 「読み取れた」とみなす。フォームには反映できても登録時のバリデーションで
    // 必ず弾かれる半端な値・値域外の値を成功扱いで返さないようにするため。
    // 脈拍は「読み取れなかった (0)」を許容しつつ、非ゼロなら値域内を要求する。
    let reading = payload.reading.filter(reading_passes_validation);

    // `readings` (手書きメモの複数行) には `reading` と同じ値域フィルタを適用しない。
    // 単発読み取りのフィルタは「フォームに反映できても登録時に弾かれる値を隠す」ためのもの
    // だが、メモは行ごとにユーザーが目視で採否・修正する前提のUIになるため、値域外や
    // systolic <= diastolic の行を黙って落とすとユーザーが気付けないままデータが欠落する。
    // フロントエンド側で低信頼度の行の初期チェックを外すことで対応する。
    let readings = payload.readings;

    Ok(OcrResult {
        kind,
        raw_display: payload.raw_display,
        reading,
        readings,
        notes: payload.notes,
        usage: response.usage_metadata.usage(),
    })
}

/// Gemini が読み取った 1 行が「登録 API (`records.rs`) と共有する `validate_bp_values`
/// を満たしているか」を判定する (単発読み取りの `reading` に使う)。
/// `pulse == 0` は「読み取れなかった」を表すため `None` として扱う。
fn reading_passes_validation(r: &OcrReading) -> bool {
    let pulse = (r.pulse != 0).then_some(r.pulse);
    validate_bp_values(r.systolic, r.diastolic, pulse).is_ok()
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct OcrPayload {
    #[serde(default)]
    kind: String,
    #[serde(default, rename = "rawDisplay")]
    raw_display: Option<String>,
    #[serde(default)]
    reading: Option<OcrReading>,
    #[serde(default)]
    readings: Vec<OcrReading>,
    #[serde(default)]
    notes: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_body_asks_for_low_thinking() {
        let body = serde_json::to_value(request_body(b"image", "image/jpeg"))
            .expect("request body should serialize");
        assert_eq!(
            body["generationConfig"]["thinkingConfig"]["thinkingLevel"],
            "low"
        );
    }

    fn gemini_envelope(inner_json: &str) -> String {
        format!(
            r#"{{
                "candidates": [{{
                    "content": {{ "parts": [{{ "text": {inner} }}] }},
                    "finishReason": "STOP"
                }}],
                "usageMetadata": {{ "promptTokenCount": 100, "candidatesTokenCount": 20, "thoughtsTokenCount": 300 }}
            }}"#,
            inner = serde_json::to_string(inner_json).expect("failed to serialize test JSON")
        )
    }

    /// `dump_dir` に、今の送信内容で成功したやりとりを1つ残したサービス。
    async fn service_with_dumped_reading(
        dump_dir: &std::path::Path,
        image: &[u8],
        reuse: bool,
    ) -> OcrService {
        let service = OcrService::with_api_key("test-key")
            .with_dump_dir(Some(dump_dir.to_path_buf()))
            .with_reuse_dumps(reuse);
        let raw = gemini_envelope(
            r#"{"kind":"monitor_bp","reading":{"systolic":128,"diastolic":82,"pulse":70,"confidence":0.95}}"#,
        );
        ocr_dump::save(
            dump_dir.to_path_buf(),
            ocr_dump::Exchange {
                model: service.model.clone(),
                request: serde_json::to_value(request_body(image, "image/jpeg"))
                    .expect("the request should be JSON"),
                outcome: Ok((200, raw)),
                elapsed: Duration::from_millis(10),
            },
        )
        .await;
        service
    }

    #[tokio::test]
    async fn reuses_the_dumped_response_to_the_same_image() {
        let tmp = crate::test_support::TempDir::new("reuse-same");
        let service = service_with_dumped_reading(tmp.path(), &[1, 2, 3], true).await;

        let result = service
            .reuse_dumped(&[1, 2, 3], "image/jpeg")
            .await
            .expect("the dumped response should be reused");
        assert_eq!(
            result.reading.expect("reading should be present").systolic,
            128
        );
        assert!(
            service
                .reuse_dumped(&[9, 9, 9], "image/jpeg")
                .await
                .is_none()
        );
    }

    #[tokio::test]
    async fn does_not_reuse_dumps_unless_enabled() {
        let tmp = crate::test_support::TempDir::new("reuse-off");
        let service = service_with_dumped_reading(tmp.path(), &[1, 2, 3], false).await;

        assert!(
            service
                .reuse_dumped(&[1, 2, 3], "image/jpeg")
                .await
                .is_none()
        );
    }

    #[test]
    fn parses_monitor_bp_reading() {
        let raw = gemini_envelope(
            r#"{"kind":"monitor_bp","rawDisplay":"128 82 70","reading":{"systolic":128,"diastolic":82,"pulse":70,"confidence":0.95}}"#,
        );
        let result = parse_gemini_response(&raw).expect("parse_gemini_response should not error");
        assert_eq!(result.kind, "monitor_bp");
        let reading = result.reading.expect("reading should be present");
        assert_eq!(reading.systolic, 128);
        assert_eq!(reading.diastolic, 82);
        assert_eq!(reading.pulse, 70);
        assert_eq!(result.usage.input_tokens, 100);
        // 思考のトークンも出力として課金されるので足す。
        assert_eq!(result.usage.output_tokens, 320);
    }

    #[test]
    fn monitor_none_has_no_reading() {
        let raw = gemini_envelope(r#"{"kind":"monitor_none","notes":"時刻を表示中"}"#);
        let result = parse_gemini_response(&raw).expect("parse_gemini_response should not error");
        assert_eq!(result.kind, "monitor_none");
        assert!(result.reading.is_none());
        assert_eq!(result.notes.as_deref(), Some("時刻を表示中"));
    }

    #[test]
    fn empty_kind_falls_back_to_unknown() {
        let raw = gemini_envelope(r#"{}"#);
        let result = parse_gemini_response(&raw).expect("parse_gemini_response should not error");
        assert_eq!(result.kind, "unknown");
    }

    #[test]
    fn overflowing_number_clamps_to_zero_and_reading_is_rejected() {
        // PoC のコメントにある「144 が 1.44e20 として返る」ケースの再現。
        // systolic は (エラーにせず) 0 にクランプされるが、その結果 systolic > diastolic
        // の関係が崩れるため、reading 自体は不採用になる (パニックしないことも合わせて確認)。
        let raw = gemini_envelope(
            r#"{"kind":"monitor_bp","reading":{"systolic":1.44e20,"diastolic":82,"pulse":70,"confidence":0.9}}"#,
        );
        let result = parse_gemini_response(&raw).expect("parse_gemini_response should not error");
        assert!(result.reading.is_none());
    }

    #[test]
    fn reading_is_rejected_when_systolic_is_not_greater_than_diastolic() {
        // 血圧の定義上、収縮期は必ず拡張期より高い。この関係が崩れている数値は
        // 読み取り自体が誤っている可能性が高いため、reading を採用しない。
        let raw = gemini_envelope(
            r#"{"kind":"monitor_bp","reading":{"systolic":80,"diastolic":90,"pulse":70,"confidence":0.5}}"#,
        );
        let result = parse_gemini_response(&raw).expect("parse_gemini_response should not error");
        assert!(result.reading.is_none());
    }

    #[test]
    fn reading_is_rejected_when_out_of_the_registration_api_range() {
        // 登録 API (records.rs) が拒否する値域外の数値は、フォームに反映できても
        // 登録時に必ず弾かれてしまうため、OCR の時点で不採用にする。
        let raw = gemini_envelope(
            r#"{"kind":"monitor_bp","reading":{"systolic":300,"diastolic":200,"pulse":70,"confidence":0.9}}"#,
        );
        let result = parse_gemini_response(&raw).expect("parse_gemini_response should not error");
        assert!(result.reading.is_none());
    }

    #[test]
    fn reading_is_rejected_when_pulse_is_out_of_range_but_nonzero() {
        // pulse は「読み取れなかった (0)」だけを許容する。非ゼロの値域外
        // (例えば誤読で3桁になった脈拍) は不採用にする。
        let raw = gemini_envelope(
            r#"{"kind":"monitor_bp","reading":{"systolic":128,"diastolic":82,"pulse":9999,"confidence":0.9}}"#,
        );
        let result = parse_gemini_response(&raw).expect("parse_gemini_response should not error");
        assert!(result.reading.is_none());
    }

    #[test]
    fn string_typed_numbers_are_parsed() {
        let raw = gemini_envelope(
            r#"{"kind":"monitor_bp","reading":{"systolic":"128","diastolic":"82","pulse":"70","confidence":0.9}}"#,
        );
        let result = parse_gemini_response(&raw).expect("parse_gemini_response should not error");
        let reading = result.reading.expect("reading should be present");
        assert_eq!(reading.systolic, 128);
        assert_eq!(reading.diastolic, 82);
        assert_eq!(reading.pulse, 70);
    }

    #[test]
    fn unparsable_string_number_clamps_to_zero() {
        // `deserialize_flex_int` 自体の挙動を確認する (systolic > diastolic の
        // フィルタとは独立させるため `OcrReading` を直接デコードする)。
        let reading: OcrReading = serde_json::from_str(
            r#"{"systolic":"???","diastolic":82,"pulse":70,"confidence":0.2}"#,
        )
        .expect("failed to deserialize OcrReading");
        assert_eq!(reading.systolic, 0);
        assert_eq!(reading.diastolic, 82);
    }

    #[test]
    fn missing_numeric_field_in_reading_defaults_to_zero() {
        // Gemini がキーの欠けた行を返しても、1行の欠落で手書きメモの他の行も含めた
        // 応答全体を失敗させないよう、欠けた値は 0 として読む。
        let reading: OcrReading = serde_json::from_str(r#"{"systolic":128,"pulse":70}"#)
            .expect("failed to deserialize OcrReading");
        assert_eq!(reading.systolic, 128);
        assert_eq!(reading.diastolic, 0);
        assert_eq!(reading.pulse, 70);
        assert_eq!(reading.confidence, 0.0);
    }

    #[test]
    fn memo_row_with_missing_field_does_not_fail_the_whole_batch() {
        let raw = gemini_envelope(
            r#"{"kind":"memo","readings":[
                {"measuredOn":"09-05","systolic":128,"diastolic":82,"pulse":70,"confidence":0.9},
                {"measuredOn":"09-05","systolic":130,"pulse":68,"confidence":0.9}
            ]}"#,
        );
        let result = parse_gemini_response(&raw).expect("parse_gemini_response should not error");
        assert_eq!(result.readings.len(), 2);
        assert_eq!(result.readings[1].diastolic, 0);
    }

    #[test]
    fn empty_candidates_is_a_parse_error() {
        let raw = r#"{"candidates": [], "usageMetadata": {}}"#;
        assert!(matches!(
            parse_gemini_response(raw),
            Err(OcrError::ParseResponse { .. })
        ));
    }

    #[test]
    fn a_malformed_candidate_still_reports_the_usage() {
        let raw = r#"{"candidates": "oops", "usageMetadata": {"promptTokenCount": 7, "candidatesTokenCount": i64::MAX}}"#
            .replace("i64::MAX", &i64::MAX.to_string());
        let usage = match parse_gemini_response(&raw) {
            Err(OcrError::ParseResponse { usage: Some(usage) }) => usage,
            other => panic!("unexpected: {other:?}"),
        };
        assert_eq!(usage.input_tokens, 7);
        assert_eq!(usage.output_tokens, i64::MAX);
    }

    #[test]
    fn malformed_inner_json_is_a_parse_error() {
        let raw = gemini_envelope("this is not JSON");
        // 応答は届いているので、使用量 (課金された分) を持ち帰る。
        assert!(matches!(
            parse_gemini_response(&raw),
            Err(OcrError::ParseResponse { usage: Some(_) })
        ));
    }

    #[test]
    fn zero_reading_values_are_treated_as_no_reading() {
        // モデルが kind: monitor_bp を返しつつ数値が全く読めなかったケース。
        let raw = gemini_envelope(
            r#"{"kind":"monitor_bp","reading":{"systolic":0,"diastolic":0,"pulse":0,"confidence":0.1}}"#,
        );
        let result = parse_gemini_response(&raw).expect("parse_gemini_response should not error");
        assert!(result.reading.is_none());
    }

    #[test]
    fn parses_memo_readings() {
        let raw = gemini_envelope(
            r#"{"kind":"memo","readings":[
                {"measuredOn":"09-05","time":"07:30","systolic":128,"diastolic":82,"pulse":70,"confidence":0.92,"memo":"頭痛あり","period":"none"},
                {"measuredOn":"09-05","time":"","systolic":130,"diastolic":85,"pulse":0,"confidence":0.6,"note":"時刻の記載なし","period":"evening"},
                {"measuredOn":"09-06","systolic":125,"diastolic":80,"pulse":0,"confidence":0.9,"period":"noon"}
            ]}"#,
        );
        let result = parse_gemini_response(&raw).expect("parse_gemini_response should not error");
        assert_eq!(result.kind, "memo");
        assert!(result.reading.is_none());
        assert_eq!(result.readings.len(), 3);
        assert_eq!(result.readings[0].measured_on.as_deref(), Some("09-05"));
        assert_eq!(result.readings[0].time.as_deref(), Some("07:30"));
        assert_eq!(result.readings[0].memo.as_deref(), Some("頭痛あり"));
        assert_eq!(result.readings[1].time.as_deref(), Some(""));
        assert_eq!(result.readings[1].note.as_deref(), Some("時刻の記載なし"));
        assert_eq!(result.readings[1].memo, None);
        // "none" と想定外の値は無し、欠けていても無し。
        assert_eq!(result.readings[0].period, None);
        assert_eq!(result.readings[1].period, Some(OcrPeriod::Evening));
        assert_eq!(result.readings[2].period, None);
    }

    #[test]
    fn memo_readings_are_not_range_filtered() {
        // `reading` (単発読み取り) と異なり、`readings` (手書きメモ) は値域外・
        // systolic <= diastolic の行でも黙って落とさず、そのまま返す
        // (ユーザーがテーブルUIで目視確認・修正する前提のため)。
        let raw = gemini_envelope(
            r#"{"kind":"memo","readings":[
                {"measuredOn":"09-05","systolic":50,"diastolic":90,"pulse":9999,"confidence":0.2,"note":"判読困難"}
            ]}"#,
        );
        let result = parse_gemini_response(&raw).expect("parse_gemini_response should not error");
        assert_eq!(result.readings.len(), 1);
        assert_eq!(result.readings[0].systolic, 50);
        assert_eq!(result.readings[0].pulse, 9999);
    }

    #[test]
    fn missing_readings_defaults_to_empty() {
        // monitor_bp/monitor_none/unknown では readings フィールド自体が無いのが通常。
        let raw = gemini_envelope(r#"{"kind":"monitor_none","notes":"時刻を表示中"}"#);
        let result = parse_gemini_response(&raw).expect("parse_gemini_response should not error");
        assert!(result.readings.is_empty());
    }
}
