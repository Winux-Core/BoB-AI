use anyhow::Result;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConversationRecord {
    pub id: String,
    pub title: String,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageRecord {
    pub id: String,
    pub conversation_id: String,
    pub role: String,
    pub content: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkspaceProfileRecord {
    pub default_model: String,
    pub system_prompt: String,
    pub context_injection: String,
    pub personalization: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkspaceProfileUpdate {
    pub default_model: String,
    pub system_prompt: String,
    pub context_injection: String,
    pub personalization: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OllamaEndpointRecord {
    pub id: String,
    pub name: String,
    pub base_url: String,
    pub kind: String,
    pub enabled: bool,
    pub is_default: bool,
    pub auth_token: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OllamaEndpointUpsert {
    pub id: Option<String>,
    pub name: String,
    pub base_url: String,
    pub kind: String,
    pub enabled: bool,
    pub is_default: bool,
    pub auth_token: Option<String>,
    pub clear_auth_token: bool,
}

pub struct DbManager {
    _postgres_url: String,
}

impl DbManager {
    pub fn new(postgres_url: String) -> Self {
        Self { _postgres_url: postgres_url }
    }

    pub fn create_conversation(&self, _title: Option<&str>) -> Result<ConversationRecord> {
        todo!("create conversation in postgres")
    }

    pub fn list_conversations(&self, _limit: i64) -> Result<Vec<ConversationRecord>> {
        todo!("list conversations from postgres")
    }

    pub fn list_messages(&self, _conversation_id: &str, _limit: i64) -> Result<Vec<MessageRecord>> {
        todo!("list messages from postgres")
    }

    pub fn add_message(&self, _conversation_id: &str, _role: &str, _content: &str) -> Result<MessageRecord> {
        todo!("add message to postgres")
    }

    pub fn get_workspace_profile(&self) -> Result<WorkspaceProfileRecord> {
        todo!("get workspace profile from postgres")
    }

    pub fn save_workspace_profile(&self, _update: WorkspaceProfileUpdate) -> Result<WorkspaceProfileRecord> {
        todo!("save workspace profile to postgres")
    }

    pub fn list_ollama_endpoints(&self) -> Result<Vec<OllamaEndpointRecord>> {
        todo!("list ollama endpoints from postgres")
    }

    pub fn upsert_ollama_endpoint(&self, _input: OllamaEndpointUpsert) -> Result<OllamaEndpointRecord> {
        todo!("upsert ollama endpoint in postgres")
    }

    pub fn delete_ollama_endpoint(&self, _id: &str) -> Result<()> {
        todo!("delete ollama endpoint from postgres")
    }

    pub fn set_default_ollama_endpoint(&self, _id: &str) -> Result<OllamaEndpointRecord> {
        todo!("set default ollama endpoint in postgres")
    }

    pub fn get_ollama_endpoint(&self, _id: &str) -> Result<OllamaEndpointRecord> {
        todo!("get ollama endpoint from postgres")
    }

    pub fn resolve_ollama_endpoint(&self, _id: Option<&str>, _fallback_url: &str) -> Result<OllamaEndpointRecord> {
        todo!("resolve ollama endpoint from postgres or fallback")
    }
}
