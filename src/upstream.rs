//! 外部 HTTP API の応答を読む共通処理。

use std::time::Duration;

/// ログイン・決済のエンドポイントとの単純な往復なので、OCR (90s) よりずっと短くてよい。
const HTTP_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("failed to call upstream service")]
    Request(#[source] reqwest::Error),
    #[error("upstream service returned status {0}")]
    UpstreamStatus(u16),
    #[error("failed to parse upstream service response")]
    ParseResponse,
}

pub fn http_client() -> reqwest::Client {
    reqwest::Client::builder()
        .timeout(HTTP_TIMEOUT)
        .build()
        .expect("failed to build reqwest client")
}

/// 応答のステータスと本文を、ステータス判定の前にまとめて読む。
pub async fn response_body(
    response: reqwest::Response,
) -> Result<(reqwest::StatusCode, String), reqwest::Error> {
    let status = response.status();
    let body = response.text().await?;
    Ok((status, body))
}

/// 成功応答の本文を返す。`provider`・`endpoint` はログで呼び出し元を見分けるためのもの。
pub async fn success_body(
    response: reqwest::Response,
    provider: &'static str,
    endpoint: &'static str,
) -> Result<String, Error> {
    // `without_url()`: エラーメッセージに URL を含めない (ログに出るため)。
    let (status, body) = response_body(response)
        .await
        .map_err(|err| Error::Request(err.without_url()))?;
    if !status.is_success() {
        tracing::warn!(status = %status, provider, endpoint, "外部の API がエラーを返しました");
        return Err(Error::UpstreamStatus(status.as_u16()));
    }
    Ok(body)
}

/// 成功応答の本文を JSON として読む。
pub async fn parse_success<T: serde::de::DeserializeOwned>(
    response: reqwest::Response,
    provider: &'static str,
    endpoint: &'static str,
) -> Result<T, Error> {
    let body = success_body(response, provider, endpoint).await?;
    serde_json::from_str(&body).map_err(|_| Error::ParseResponse)
}
