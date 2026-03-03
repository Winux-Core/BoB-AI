use anyhow::Result;
use serde::{Deserialize, Serialize};
use crate::error::BobError;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OllamaGenerateResponse {
    pub model: String,
    pub response: String,
    pub done: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prompt_eval_count: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub eval_count: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total_duration: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OllamaStreamChunk {
    pub model: String,
    pub response: String,
    pub done: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prompt_eval_count: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub eval_count: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total_duration: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OllamaModelInfo {
    pub name: String,
    pub model: String,
}

#[derive(Debug, Deserialize)]
struct OllamaTagsResponse {
    models: Vec<OllamaModelInfo>,
}

pub fn generate(
    base_url: &str,
    model: &str,
    prompt: &str,
    system: Option<String>,
    auth_token: Option<&str>,
) -> Result<OllamaGenerateResponse> {
    let url = format!("{}/api/generate", base_url.trim_end_matches('/'));
    let mut body = serde_json::json!({
        "model": model,
        "prompt": prompt,
        "stream": false,
    });
    if let Some(sys) = system {
        body["system"] = serde_json::Value::String(sys);
    }

    let mut req = ureq::post(&url);
    if let Some(token) = auth_token {
        req = req.set("Authorization", &format!("Bearer {}", token));
    }
    let resp = req.send_json(&body)?;
    let result: OllamaGenerateResponse = resp.into_json()?;
    Ok(result)
}

pub async fn generate_stream(
    base_url: &str,
    model: &str,
    prompt: &str,
    system: Option<String>,
    auth_token: Option<&str>,
) -> Result<impl futures::stream::Stream<Item = Result<OllamaStreamChunk, BobError>>, BobError> {
    let url = format!("{}/api/generate", base_url.trim_end_matches('/'));
    let mut body = serde_json::json!({
        "model": model,
        "prompt": prompt,
        "stream": true,
    });
    if let Some(sys) = system {
        body["system"] = serde_json::Value::String(sys);
    }

    let client = reqwest::Client::new();
    let mut req = client.post(&url).json(&body);
    if let Some(token) = auth_token {
        req = req.bearer_auth(token);
    }

    let response = req.send().await.map_err(|e| BobError::Ollama(e.to_string()))?;
    if !response.status().is_success() {
        let status = response.status();
        let text = response.text().await.unwrap_or_default();
        return Err(BobError::Ollama(format!("ollama returned {}: {}", status, text)));
    }

    let byte_stream = response.bytes_stream();

    Ok(async_stream::stream! {
        use futures::StreamExt;
        let mut buffer = String::new();
        tokio::pin!(byte_stream);
        while let Some(chunk_result) = byte_stream.next().await {
            match chunk_result {
                Ok(bytes) => {
                    buffer.push_str(&String::from_utf8_lossy(&bytes));
                    while let Some(newline_pos) = buffer.find('\n') {
                        let line = buffer[..newline_pos].trim().to_string();
                        buffer = buffer[newline_pos + 1..].to_string();
                        if line.is_empty() {
                            continue;
                        }
                        match serde_json::from_str::<OllamaStreamChunk>(&line) {
                            Ok(chunk) => yield Ok(chunk),
                            Err(e) => yield Err(BobError::Ollama(format!("failed to parse chunk: {}", e))),
                        }
                    }
                }
                Err(e) => {
                    yield Err(BobError::Ollama(format!("stream error: {}", e)));
                    break;
                }
            }
        }
    })
}

pub fn list_models(base_url: &str, auth_token: Option<&str>) -> Result<Vec<OllamaModelInfo>> {
    let url = format!("{}/api/tags", base_url.trim_end_matches('/'));
    let mut req = ureq::get(&url);
    if let Some(token) = auth_token {
        req = req.set("Authorization", &format!("Bearer {}", token));
    }
    let resp = req.call()?;
    let tags: OllamaTagsResponse = resp.into_json()?;
    Ok(tags.models)
}
