use anyhow::Result;
use serde::{Deserialize, Serialize};

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
