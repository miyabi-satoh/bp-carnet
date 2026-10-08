//! Gemini とのやりとり (送った写真・プロンプトと、返ってきた応答) をファイルに残す (docs/ocr.md)。
//! `[ocr] dump_dir` を設定したときだけ使う。開発用に、残したやりとりから同じ送信内容の応答を探して使い回せる
//! (`[ocr] reuse_dumps`、→ [`find_response`])。

use std::path::{Path, PathBuf};
use std::time::Duration;

use serde_json::{Value, json};

use crate::config::create_owner_only_dir;

/// `generateContent` の1回分。
pub struct Exchange {
    pub model: String,
    /// 送った本文 (`GeminiRequest` を JSON にしたもの)。
    pub request: Value,
    /// 応答のステータスと本文。通信自体に失敗したらそのエラー。
    pub outcome: Result<(u16, String), String>,
    pub elapsed: Duration,
}

/// `root` の下に1回分のディレクトリを作って書き出す。残せなくても読み取りは続けられるので、失敗はログに出すだけにする。
pub async fn save(root: PathBuf, exchange: Exchange) {
    let saved = tokio::task::spawn_blocking(move || write(&root, exchange))
        .await
        .map_err(std::io::Error::other)
        .and_then(|result| result);
    match saved {
        Ok(dir) => tracing::debug!(dir = %dir.display(), "Gemini とのやりとりを保存しました"),
        Err(err) => {
            tracing::warn!(error = %crate::error_chain_line(&err), "Gemini とのやりとりを保存できませんでした")
        }
    }
}

fn write(root: &Path, exchange: Exchange) -> std::io::Result<PathBuf> {
    // 名前で時刻順に並ぶようにし、同じミリ秒の呼び出しとは乱数で区別する。
    let now = jiff::Timestamp::now();
    let dir = root.join(format!(
        "{}-{}",
        now.strftime("%Y%m%dT%H%M%S%.3fZ"),
        crate::token::random_url_safe(3)
    ));
    create_owner_only_dir(&dir)?;

    let (request, images) = split_images(exchange.request)?;
    for (file_name, bytes) in &images {
        std::fs::write(dir.join(file_name), bytes)?;
    }
    std::fs::write(
        dir.join("request.json"),
        serde_json::to_vec_pretty(&request).map_err(std::io::Error::other)?,
    )?;

    let (status, error) = match exchange.outcome {
        Ok((status, body)) => {
            std::fs::write(dir.join("response.json"), body)?;
            (Some(status), None)
        }
        Err(err) => (None, Some(err)),
    };
    let meta = json!({
        "model": exchange.model,
        "savedAt": now.to_string(),
        "elapsedMs": u64::try_from(exchange.elapsed.as_millis()).unwrap_or(u64::MAX),
        "status": status,
        "error": error,
    });
    std::fs::write(
        dir.join("meta.json"),
        serde_json::to_vec_pretty(&meta).map_err(std::io::Error::other)?,
    )?;
    Ok(dir)
}

/// 本文から取り出した写真。書き出すファイル名と中身。
type Images = Vec<(String, Vec<u8>)>;

/// 送った本文から写真を取り出す。写真の部分を書き出すファイル名に置き換えた本文と、取り出した写真を返す。
/// 数 MB の base64 を request.json に重ねて残さないため。
fn split_images(mut request: Value) -> std::io::Result<(Value, Images)> {
    let mut images = Vec::new();
    let parts = request
        .get_mut("contents")
        .and_then(Value::as_array_mut)
        .into_iter()
        .flatten()
        .filter_map(|content| content.get_mut("parts").and_then(Value::as_array_mut))
        .flatten();
    for part in parts {
        let Some(inline) = part.get_mut("inline_data") else {
            continue;
        };
        let file_name = format!(
            "image-{}.{}",
            images.len(),
            extension(
                inline
                    .get("mime_type")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
            )
        );
        let Some(Value::String(data)) = inline.get_mut("data") else {
            continue;
        };
        let bytes =
            base64::Engine::decode(&base64::engine::general_purpose::STANDARD, data.as_bytes())
                .map_err(std::io::Error::other)?;
        *data = file_name.clone();
        images.push((file_name, bytes));
    }
    Ok((request, images))
}

/// `root` の下から、`model` に `request` を送って成功したやりとりを新しい順に探し、その応答の本文を返す。
/// 探せなくても Gemini を呼べば済むので、失敗はログに出して `None` にする。
pub async fn find_response(root: PathBuf, model: String, request: Value) -> Option<String> {
    let found = tokio::task::spawn_blocking(move || find(&root, &model, request))
        .await
        .map_err(std::io::Error::other)
        .and_then(|result| result);
    match found {
        Ok(found) => found,
        Err(err) => {
            tracing::warn!(error = %crate::error_chain_line(&err), "残したやりとりを探せませんでした");
            None
        }
    }
}

fn find(root: &Path, model: &str, request: Value) -> std::io::Result<Option<String>> {
    let (request, images) = split_images(request)?;
    let entries = match std::fs::read_dir(root) {
        Ok(entries) => entries,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(err) => return Err(err),
    };
    let mut dirs: Vec<PathBuf> = entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.is_dir())
        .collect();
    // 名前が時刻順なので、逆に並べて新しいものから見る。
    dirs.sort_unstable_by(|a, b| b.cmp(a));
    for dir in dirs {
        // 読めないもの (書きかけ・手で消したファイルなど) は、合わないものとして飛ばす。
        if matches(&dir, model, &request, &images).unwrap_or(false) {
            return std::fs::read_to_string(dir.join("response.json")).map(Some);
        }
    }
    Ok(None)
}

/// `dir` のやりとりが、同じモデル・同じ送信内容 (写真・プロンプト・生成の設定) で成功したものか。
fn matches(dir: &Path, model: &str, request: &Value, images: &Images) -> std::io::Result<bool> {
    let meta = read_json(&dir.join("meta.json"))?;
    if meta["status"] != 200 || meta["model"] != model {
        return Ok(false);
    }
    if read_json(&dir.join("request.json"))? != *request {
        return Ok(false);
    }
    for (file_name, bytes) in images {
        if std::fs::read(dir.join(file_name))? != *bytes {
            return Ok(false);
        }
    }
    Ok(true)
}

fn read_json(path: &Path) -> std::io::Result<Value> {
    serde_json::from_slice(&std::fs::read(path)?).map_err(std::io::Error::other)
}

/// 写真のファイル名に付ける拡張子。MIME タイプはクライアントが送ったものなので、ファイル名に使えない文字を含むものは使わない。
fn extension(mime_type: &str) -> &str {
    match mime_type {
        "image/jpeg" => "jpg",
        other => other
            .strip_prefix("image/")
            .filter(|ext| !ext.is_empty() && ext.chars().all(|c| c.is_ascii_alphanumeric()))
            .unwrap_or("bin"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::TempDir;

    fn exchange(outcome: Result<(u16, String), String>) -> Exchange {
        Exchange {
            model: "gemini-test".to_string(),
            request: json!({
                "contents": [{"parts": [
                    {"inline_data": {"mime_type": "image/jpeg", "data": "AQID"}},
                    {"text": "prompt"}
                ]}],
                "generationConfig": {"temperature": 0.0}
            }),
            outcome,
            elapsed: Duration::from_millis(1500),
        }
    }

    fn read_json(path: &Path) -> Value {
        serde_json::from_slice(&std::fs::read(path).expect("the file should exist"))
            .expect("the file should be JSON")
    }

    #[test]
    fn writes_the_image_apart_from_the_request_and_keeps_the_raw_response() {
        let tmp = TempDir::new("ok");
        let dir = write(
            tmp.path(),
            exchange(Ok((200, r#"{"candidates":[]}"#.to_string()))),
        )
        .expect("write should succeed");

        assert_eq!(
            std::fs::read(dir.join("image-0.jpg")).expect("the image should be written"),
            [1, 2, 3]
        );
        let request = read_json(&dir.join("request.json"));
        assert_eq!(
            request["contents"][0]["parts"][0]["inline_data"]["data"],
            "image-0.jpg"
        );
        assert_eq!(request["contents"][0]["parts"][1]["text"], "prompt");
        assert_eq!(
            std::fs::read_to_string(dir.join("response.json"))
                .expect("the response should be written"),
            r#"{"candidates":[]}"#
        );
        let meta = read_json(&dir.join("meta.json"));
        assert_eq!(meta["status"], 200);
        assert_eq!(meta["elapsedMs"], 1500);
    }

    #[test]
    fn records_a_transport_error_without_a_response_file() {
        let tmp = TempDir::new("error");
        let dir = write(tmp.path(), exchange(Err("timed out".to_string())))
            .expect("write should succeed");

        assert!(!dir.join("response.json").exists());
        let meta = read_json(&dir.join("meta.json"));
        assert_eq!(meta["error"], "timed out");
        assert_eq!(meta["status"], Value::Null);
    }

    fn find_in(root: &Path, model: &str, request: Value) -> Option<String> {
        find(root, model, request).expect("find should not fail")
    }

    #[test]
    fn finds_the_response_of_the_same_request_to_the_same_model() {
        let tmp = TempDir::new("find-same");
        let body = r#"{"candidates":[]}"#;
        write(tmp.path(), exchange(Ok((200, body.to_string())))).expect("write should succeed");

        let request = exchange(Ok((200, String::new()))).request;
        assert_eq!(
            find_in(tmp.path(), "gemini-test", request).as_deref(),
            Some(body)
        );
    }

    #[test]
    fn does_not_reuse_a_response_to_a_different_image_prompt_or_model() {
        let tmp = TempDir::new("find-different");
        write(
            tmp.path(),
            exchange(Ok((200, r#"{"candidates":[]}"#.to_string()))),
        )
        .expect("write should succeed");
        let request = || exchange(Ok((200, String::new()))).request;

        let mut other_image = request();
        other_image["contents"][0]["parts"][0]["inline_data"]["data"] = json!("AQIE");
        assert_eq!(find_in(tmp.path(), "gemini-test", other_image), None);

        let mut other_prompt = request();
        other_prompt["contents"][0]["parts"][1]["text"] = json!("new prompt");
        assert_eq!(find_in(tmp.path(), "gemini-test", other_prompt), None);

        assert_eq!(find_in(tmp.path(), "gemini-other", request()), None);
    }

    #[test]
    fn does_not_reuse_a_failed_exchange() {
        let tmp = TempDir::new("find-failed");
        write(tmp.path(), exchange(Ok((503, "busy".to_string())))).expect("write should succeed");
        write(tmp.path(), exchange(Err("timed out".to_string()))).expect("write should succeed");

        let request = exchange(Ok((200, String::new()))).request;
        assert_eq!(find_in(tmp.path(), "gemini-test", request), None);
    }

    #[test]
    fn finds_nothing_when_the_dump_dir_does_not_exist_yet() {
        let tmp = TempDir::new("find-missing");
        let request = exchange(Ok((200, String::new()))).request;
        assert_eq!(
            find_in(&tmp.path().join("missing"), "gemini-test", request),
            None
        );
    }

    #[test]
    fn extension_ignores_mime_types_unsafe_for_file_names() {
        assert_eq!(extension("image/png"), "png");
        assert_eq!(extension("image/../x"), "bin");
        assert_eq!(extension("text/plain"), "bin");
    }
}
