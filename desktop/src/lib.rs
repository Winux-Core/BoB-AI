use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use bob_core::ollama::OllamaGenerateResponse;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::json;

#[derive(Debug, Serialize, Deserialize, Clone)]
struct ConversationRecord {
    id: String,
    title: String,
    created_at: String,
    updated_at: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct MessageRecord {
    id: String,
    conversation_id: String,
    role: String,
    content: String,
    created_at: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct ConversationReplyResponse {
    conversation_id: String,
    user_message: MessageRecord,
    assistant_message: MessageRecord,
    model_response: OllamaGenerateResponse,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(default)]
struct GuiSettings {
    api_base_url: String,
    api_token: String,
    default_model: String,
    system_prompt: String,
}

impl Default for GuiSettings {
    fn default() -> Self {
        Self {
            api_base_url: "http://127.0.0.1:8787".to_string(),
            api_token: String::new(),
            default_model: "llama3.1".to_string(),
            system_prompt: "You are BoB, concise and practical.".to_string(),
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(default)]
struct LocalProfile {
    settings: GuiSettings,
    context_injection: String,
    personalization: serde_json::Value,
}

impl Default for LocalProfile {
    fn default() -> Self {
        Self {
            settings: GuiSettings::default(),
            context_injection: String::new(),
            personalization: json!({}),
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct CachedConversation {
    conversation_id: String,
    title: Option<String>,
    updated_at_unix_ms: u64,
    messages: Vec<MessageRecord>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct CachedChatSummary {
    conversation_id: String,
    title: Option<String>,
    updated_at_unix_ms: u64,
    message_count: usize,
}

#[derive(Debug, Serialize)]
struct DesktopPaths {
    config_dir: String,
    chats_dir: String,
    gui_settings_path: String,
    context_injection_path: String,
    personalization_path: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct ApiErrorBody {
    error: String,
}

#[tauri::command]
fn desktop_paths_cmd() -> Result<DesktopPaths, String> {
    Ok(DesktopPaths {
        config_dir: bob_config_dir().display().to_string(),
        chats_dir: chats_cache_dir().display().to_string(),
        gui_settings_path: gui_settings_path().display().to_string(),
        context_injection_path: context_injection_path().display().to_string(),
        personalization_path: personalization_path().display().to_string(),
    })
}

#[tauri::command]
fn load_local_profile_cmd() -> Result<LocalProfile, String> {
    ensure_local_dirs()?;

    let mut profile = LocalProfile::default();
    if let Some(saved) = read_json_file::<GuiSettings>(&gui_settings_path())? {
        profile.settings = saved;
    }

    if context_injection_path().exists() {
        profile.context_injection = fs::read_to_string(context_injection_path())
            .map_err(|e| format!("failed reading context injection: {}", e))?;
    }

    if let Some(saved) = read_json_file::<serde_json::Value>(&personalization_path())? {
        profile.personalization = saved;
    }

    Ok(profile)
}

#[tauri::command]
fn save_local_profile_cmd(profile: LocalProfile) -> Result<LocalProfile, String> {
    ensure_local_dirs()?;
    write_json_file(&gui_settings_path(), &profile.settings)?;
    fs::write(
        context_injection_path(),
        profile.context_injection.as_bytes(),
    )
    .map_err(|e| format!("failed writing context injection: {}", e))?;
    write_json_file(&personalization_path(), &profile.personalization)?;
    Ok(profile)
}

#[tauri::command]
fn api_start_conversation_cmd(
    api_base_url: String,
    api_token: Option<String>,
    title: Option<String>,
) -> Result<ConversationRecord, String> {
    api_post(
        &api_base_url,
        api_token,
        "/conversations",
        &json!({ "title": title }),
    )
}

#[tauri::command]
fn api_list_conversations_cmd(
    api_base_url: String,
    api_token: Option<String>,
    limit: Option<i64>,
) -> Result<Vec<ConversationRecord>, String> {
    let limit = limit.unwrap_or(50).clamp(1, 500);
    let path = format!("/conversations?limit={}", limit);
    api_get(&api_base_url, api_token, &path)
}

#[tauri::command]
fn api_get_messages_cmd(
    api_base_url: String,
    api_token: Option<String>,
    conversation_id: String,
    limit: Option<i64>,
) -> Result<Vec<MessageRecord>, String> {
    let conversation_id = normalize_conversation_id(&conversation_id)?;
    let limit = limit.unwrap_or(250).clamp(1, 500);
    let path = format!(
        "/conversations/{}/messages?limit={}",
        conversation_id, limit
    );
    api_get(&api_base_url, api_token, &path)
}

#[tauri::command]
fn api_send_message_cmd(
    api_base_url: String,
    api_token: Option<String>,
    conversation_id: String,
    model: String,
    message: String,
    system: Option<String>,
    context_injection: Option<String>,
    personalization: Option<serde_json::Value>,
    history_limit: Option<i64>,
) -> Result<ConversationReplyResponse, String> {
    let conversation_id = normalize_conversation_id(&conversation_id)?;
    let payload = json!({
        "model": model,
        "message": message,
        "system": system,
        "context_injection": context_injection,
        "personalization": personalization,
        "history_limit": history_limit
    });
    let path = format!("/conversations/{}/messages", conversation_id);
    api_post(&api_base_url, api_token, &path, &payload)
}

#[tauri::command]
fn sync_chat_cache_cmd(
    api_base_url: String,
    api_token: Option<String>,
    conversation_id: String,
    title: Option<String>,
    limit: Option<i64>,
) -> Result<CachedConversation, String> {
    let messages = api_get_messages_cmd(api_base_url, api_token, conversation_id.clone(), limit)?;
    let cached = CachedConversation {
        conversation_id: normalize_conversation_id(&conversation_id)?,
        title,
        updated_at_unix_ms: now_ms(),
        messages,
    };
    save_cached_chat(&cached)?;
    Ok(cached)
}

#[tauri::command]
fn save_cached_chat_cmd(cached: CachedConversation) -> Result<CachedConversation, String> {
    let mut normalized = cached;
    normalized.conversation_id = normalize_conversation_id(&normalized.conversation_id)?;
    normalized.updated_at_unix_ms = now_ms();
    save_cached_chat(&normalized)?;
    Ok(normalized)
}

#[tauri::command]
fn load_cached_chat_cmd(conversation_id: String) -> Result<Option<CachedConversation>, String> {
    let id = normalize_conversation_id(&conversation_id)?;
    let path = chat_cache_file(&id);
    if !path.exists() {
        return Ok(None);
    }
    read_json_file(&path)
}

#[tauri::command]
fn list_cached_chats_cmd() -> Result<Vec<CachedChatSummary>, String> {
    ensure_local_dirs()?;
    let mut output = Vec::new();
    let entries = fs::read_dir(chats_cache_dir())
        .map_err(|e| format!("failed to read chats cache dir: {}", e))?;

    for entry in entries {
        let entry = entry.map_err(|e| format!("failed to read cache entry: {}", e))?;
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        if path.extension().and_then(|ext| ext.to_str()) != Some("json") {
            continue;
        }

        if let Some(cached) = read_json_file::<CachedConversation>(&path)? {
            output.push(CachedChatSummary {
                conversation_id: cached.conversation_id,
                title: cached.title,
                updated_at_unix_ms: cached.updated_at_unix_ms,
                message_count: cached.messages.len(),
            });
        }
    }

    output.sort_by(|a, b| b.updated_at_unix_ms.cmp(&a.updated_at_unix_ms));
    Ok(output)
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct ApiConnectionStatus {
    reachable: bool,
    url: String,
    error: Option<String>,
}

#[tauri::command]
fn test_api_connection_cmd(api_base_url: String, api_token: Option<String>) -> ApiConnectionStatus {
    let url = format!("{}/healthz", api_base_url.trim_end_matches('/'));
    let mut req = ureq::get(&url);
    if let Some(token) = normalize_optional_token(api_token) {
        req = req.set("x-api-key", &token);
    }
    match req.call() {
        Ok(_) => ApiConnectionStatus {
            reachable: true,
            url: api_base_url,
            error: None,
        },
        Err(e) => ApiConnectionStatus {
            reachable: false,
            url: api_base_url,
            error: Some(map_ureq_error(e)),
        },
    }
}

#[tauri::command]
fn auto_discover_api_cmd() -> ApiConnectionStatus {
    let candidates = [
        "http://127.0.0.1:8787",
        "http://localhost:8787",
        "http://0.0.0.0:8787",
    ];
    for base_url in candidates {
        let url = format!("{}/healthz", base_url);
        if let Ok(_) = ureq::get(&url).timeout(std::time::Duration::from_secs(2)).call() {
            return ApiConnectionStatus {
                reachable: true,
                url: base_url.to_string(),
                error: None,
            };
        }
    }
    ApiConnectionStatus {
        reachable: false,
        url: candidates[0].to_string(),
        error: Some("No BoB API found on local ports. Configure a remote URL in Settings.".to_string()),
    }
}

pub fn run() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![
            desktop_paths_cmd,
            load_local_profile_cmd,
            save_local_profile_cmd,
            api_start_conversation_cmd,
            api_list_conversations_cmd,
            api_get_messages_cmd,
            api_send_message_cmd,
            sync_chat_cache_cmd,
            save_cached_chat_cmd,
            load_cached_chat_cmd,
            list_cached_chats_cmd,
            test_api_connection_cmd,
            auto_discover_api_cmd
        ])
        .run(tauri::generate_context!())
        .expect("error while running BoB desktop application");
}

fn api_get<T>(api_base_url: &str, api_token: Option<String>, path: &str) -> Result<T, String>
where
    T: DeserializeOwned,
{
    let url = format!("{}{}", api_base_url.trim_end_matches('/'), path);
    let request = build_get_request(&url, normalize_optional_token(api_token).as_deref());
    let response = request.call().map_err(map_ureq_error)?;
    response
        .into_json::<T>()
        .map_err(|e| format!("failed to parse JSON response: {}", e))
}

fn api_post<TReq, TRes>(
    api_base_url: &str,
    api_token: Option<String>,
    path: &str,
    payload: &TReq,
) -> Result<TRes, String>
where
    TReq: Serialize,
    TRes: DeserializeOwned,
{
    let url = format!("{}{}", api_base_url.trim_end_matches('/'), path);
    let request = build_post_request(&url, normalize_optional_token(api_token).as_deref());
    let response = request.send_json(payload).map_err(map_ureq_error)?;
    response
        .into_json::<TRes>()
        .map_err(|e| format!("failed to parse JSON response: {}", e))
}

fn build_get_request(url: &str, api_token: Option<&str>) -> ureq::Request {
    let request = ureq::get(url);
    match api_token {
        Some(token) => request.set("x-api-key", token),
        None => request,
    }
}

fn build_post_request(url: &str, api_token: Option<&str>) -> ureq::Request {
    let request = ureq::post(url);
    match api_token {
        Some(token) => request.set("x-api-key", token),
        None => request,
    }
}

fn map_ureq_error(err: ureq::Error) -> String {
    match err {
        ureq::Error::Status(code, response) => {
            if let Ok(body) = response.into_json::<ApiErrorBody>() {
                return format!("API request failed ({}): {}", code, body.error);
            }
            format!("API request failed with status {}", code)
        }
        ureq::Error::Transport(err) => format!("API request transport error: {}", err),
    }
}

fn save_cached_chat(cached: &CachedConversation) -> Result<(), String> {
    ensure_local_dirs()?;
    let path = chat_cache_file(&cached.conversation_id);
    write_json_file(&path, cached)
}

fn read_json_file<T>(path: &Path) -> Result<Option<T>, String>
where
    T: DeserializeOwned,
{
    if !path.exists() {
        return Ok(None);
    }
    let raw = fs::read(path).map_err(|e| format!("failed reading {}: {}", path.display(), e))?;
    let parsed = serde_json::from_slice(&raw)
        .map_err(|e| format!("failed parsing {}: {}", path.display(), e))?;
    Ok(Some(parsed))
}

fn write_json_file<T>(path: &Path, payload: &T) -> Result<(), String>
where
    T: Serialize,
{
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| format!("failed creating {}: {}", parent.display(), e))?;
    }
    let raw = serde_json::to_vec_pretty(payload)
        .map_err(|e| format!("failed serializing {}: {}", path.display(), e))?;
    fs::write(path, raw).map_err(|e| format!("failed writing {}: {}", path.display(), e))
}

fn ensure_local_dirs() -> Result<(), String> {
    fs::create_dir_all(bob_config_dir())
        .map_err(|e| format!("failed creating config dir: {}", e))?;
    fs::create_dir_all(chats_cache_dir())
        .map_err(|e| format!("failed creating chats dir: {}", e))?;
    Ok(())
}

fn normalize_conversation_id(raw: &str) -> Result<String, String> {
    let filtered: String = raw
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
        .collect();
    if filtered.is_empty() {
        return Err("conversation_id is required".to_string());
    }
    Ok(filtered)
}

fn chat_cache_file(conversation_id: &str) -> PathBuf {
    chats_cache_dir().join(format!("{}.json", conversation_id))
}

fn bob_config_dir() -> PathBuf {
    if let Ok(raw) = std::env::var("BOB_CONFIG_DIR") {
        return path_from_raw(&raw);
    }
    if let Ok(home) = std::env::var("HOME") {
        return PathBuf::from(home).join(".config").join("BoB");
    }
    PathBuf::from(".bob")
}

fn chats_cache_dir() -> PathBuf {
    if let Ok(raw) = std::env::var("BOB_CHAT_CACHE_DIR") {
        return path_from_raw(&raw);
    }
    bob_config_dir().join("Chats")
}

fn gui_settings_path() -> PathBuf {
    if let Ok(raw) = std::env::var("BOB_GUI_SETTINGS_PATH") {
        return path_from_raw(&raw);
    }
    bob_config_dir().join("gui-settings.json")
}

fn context_injection_path() -> PathBuf {
    if let Ok(raw) = std::env::var("BOB_CONTEXT_INJECTION_PATH") {
        return path_from_raw(&raw);
    }
    bob_config_dir().join("context-injection.txt")
}

fn personalization_path() -> PathBuf {
    if let Ok(raw) = std::env::var("BOB_PERSONALIZATION_PATH") {
        return path_from_raw(&raw);
    }
    bob_config_dir().join("personalization.json")
}

fn path_from_raw(raw: &str) -> PathBuf {
    if let Some(stripped) = raw.strip_prefix("~/") {
        if let Ok(home) = std::env::var("HOME") {
            return Path::new(&home).join(stripped);
        }
    }
    PathBuf::from(raw)
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn normalize_optional_token(token: Option<String>) -> Option<String> {
    match token {
        Some(value) if value.trim().is_empty() => None,
        Some(value) => Some(value),
        None => None,
    }
}
