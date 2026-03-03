use deadpool_postgres::Pool;
use serde::{Deserialize, Serialize};
use crate::error::BobError;

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
    pool: Pool,
}

impl DbManager {
    pub fn new(pool: Pool) -> Self {
        Self { pool }
    }

    pub async fn create_conversation(&self, title: Option<&str>) -> Result<ConversationRecord, BobError> {
        let client = self.pool.get().await?;
        let id = uuid::Uuid::new_v4().to_string();
        let title = title.unwrap_or("New Conversation");
        let row = client.query_one(
            "INSERT INTO conversations (id, title, created_at, updated_at) \
             VALUES ($1, $2, NOW(), NOW()) \
             RETURNING id, title, \
             TO_CHAR(created_at, 'YYYY-MM-DD\"T\"HH24:MI:SS\"Z\"') as created_at, \
             TO_CHAR(updated_at, 'YYYY-MM-DD\"T\"HH24:MI:SS\"Z\"') as updated_at",
            &[&id, &title],
        ).await?;
        Ok(ConversationRecord {
            id: row.get("id"),
            title: row.get("title"),
            created_at: row.get("created_at"),
            updated_at: row.get("updated_at"),
        })
    }

    pub async fn list_conversations(&self, limit: i64) -> Result<Vec<ConversationRecord>, BobError> {
        let client = self.pool.get().await?;
        let rows = client.query(
            "SELECT id, title, \
             TO_CHAR(created_at, 'YYYY-MM-DD\"T\"HH24:MI:SS\"Z\"') as created_at, \
             TO_CHAR(updated_at, 'YYYY-MM-DD\"T\"HH24:MI:SS\"Z\"') as updated_at \
             FROM conversations ORDER BY updated_at DESC LIMIT $1",
            &[&limit],
        ).await?;
        Ok(rows
            .iter()
            .map(|row| ConversationRecord {
                id: row.get("id"),
                title: row.get("title"),
                created_at: row.get("created_at"),
                updated_at: row.get("updated_at"),
            })
            .collect())
    }

    pub async fn list_messages(&self, conversation_id: &str, limit: i64) -> Result<Vec<MessageRecord>, BobError> {
        let client = self.pool.get().await?;
        let rows = client.query(
            "SELECT id, conversation_id, role, content, \
             TO_CHAR(created_at, 'YYYY-MM-DD\"T\"HH24:MI:SS\"Z\"') as created_at \
             FROM messages WHERE conversation_id = $1 \
             ORDER BY created_at ASC LIMIT $2",
            &[&conversation_id, &limit],
        ).await?;
        Ok(rows
            .iter()
            .map(|row| MessageRecord {
                id: row.get("id"),
                conversation_id: row.get("conversation_id"),
                role: row.get("role"),
                content: row.get("content"),
                created_at: row.get("created_at"),
            })
            .collect())
    }

    pub async fn add_message(&self, conversation_id: &str, role: &str, content: &str) -> Result<MessageRecord, BobError> {
        let client = self.pool.get().await?;
        let id = uuid::Uuid::new_v4().to_string();
        let row = client.query_one(
            "INSERT INTO messages (id, conversation_id, role, content, created_at) \
             VALUES ($1, $2, $3, $4, NOW()) \
             RETURNING id, conversation_id, role, content, \
             TO_CHAR(created_at, 'YYYY-MM-DD\"T\"HH24:MI:SS\"Z\"') as created_at",
            &[&id, &conversation_id, &role, &content],
        ).await?;
        client.execute(
            "UPDATE conversations SET updated_at = NOW() WHERE id = $1",
            &[&conversation_id],
        ).await?;
        Ok(MessageRecord {
            id: row.get("id"),
            conversation_id: row.get("conversation_id"),
            role: row.get("role"),
            content: row.get("content"),
            created_at: row.get("created_at"),
        })
    }

    pub async fn get_workspace_profile(&self) -> Result<WorkspaceProfileRecord, BobError> {
        let client = self.pool.get().await?;
        let rows = client.query(
            "SELECT default_model, system_prompt, context_injection, \
             personalization::TEXT as personalization \
             FROM workspace_profile LIMIT 1",
            &[],
        ).await?;
        if let Some(row) = rows.first() {
            let personalization_str: String = row.get("personalization");
            Ok(WorkspaceProfileRecord {
                default_model: row.get("default_model"),
                system_prompt: row.get("system_prompt"),
                context_injection: row.get("context_injection"),
                personalization: serde_json::from_str(&personalization_str)?,
            })
        } else {
            Ok(WorkspaceProfileRecord {
                default_model: String::new(),
                system_prompt: String::new(),
                context_injection: String::new(),
                personalization: serde_json::Value::Object(serde_json::Map::new()),
            })
        }
    }

    pub async fn save_workspace_profile(&self, update: WorkspaceProfileUpdate) -> Result<WorkspaceProfileRecord, BobError> {
        let client = self.pool.get().await?;
        let personalization_str = serde_json::to_string(&update.personalization)?;
        client.execute(
            "INSERT INTO workspace_profile (id, default_model, system_prompt, context_injection, personalization) \
             VALUES ('default', $1, $2, $3, $4::JSONB) \
             ON CONFLICT (id) DO UPDATE SET \
             default_model = EXCLUDED.default_model, \
             system_prompt = EXCLUDED.system_prompt, \
             context_injection = EXCLUDED.context_injection, \
             personalization = EXCLUDED.personalization",
            &[&update.default_model, &update.system_prompt, &update.context_injection, &personalization_str],
        ).await?;
        Ok(WorkspaceProfileRecord {
            default_model: update.default_model,
            system_prompt: update.system_prompt,
            context_injection: update.context_injection,
            personalization: update.personalization,
        })
    }

    pub async fn list_ollama_endpoints(&self) -> Result<Vec<OllamaEndpointRecord>, BobError> {
        let client = self.pool.get().await?;
        let rows = client.query(
            "SELECT id, name, base_url, kind, enabled, is_default, auth_token \
             FROM ollama_endpoints ORDER BY is_default DESC, name ASC",
            &[],
        ).await?;
        Ok(rows
            .iter()
            .map(|row| OllamaEndpointRecord {
                id: row.get("id"),
                name: row.get("name"),
                base_url: row.get("base_url"),
                kind: row.get("kind"),
                enabled: row.get("enabled"),
                is_default: row.get("is_default"),
                auth_token: row.get("auth_token"),
            })
            .collect())
    }

    pub async fn upsert_ollama_endpoint(&self, input: OllamaEndpointUpsert) -> Result<OllamaEndpointRecord, BobError> {
        let client = self.pool.get().await?;

        if input.is_default {
            client.execute("UPDATE ollama_endpoints SET is_default = false", &[]).await?;
        }

        let auth_token = if input.clear_auth_token {
            None
        } else {
            input.auth_token.clone()
        };

        let row = if let Some(existing_id) = &input.id {
            if input.clear_auth_token {
                client.query_one(
                    "UPDATE ollama_endpoints SET name = $1, base_url = $2, kind = $3, \
                     enabled = $4, is_default = $5, auth_token = NULL \
                     WHERE id = $6 \
                     RETURNING id, name, base_url, kind, enabled, is_default, auth_token",
                    &[&input.name, &input.base_url, &input.kind, &input.enabled, &input.is_default, existing_id],
                ).await?
            } else {
                client.query_one(
                    "UPDATE ollama_endpoints SET name = $1, base_url = $2, kind = $3, \
                     enabled = $4, is_default = $5, auth_token = COALESCE($6, auth_token) \
                     WHERE id = $7 \
                     RETURNING id, name, base_url, kind, enabled, is_default, auth_token",
                    &[&input.name, &input.base_url, &input.kind, &input.enabled, &input.is_default, &auth_token, existing_id],
                ).await?
            }
        } else {
            let id = uuid::Uuid::new_v4().to_string();
            client.query_one(
                "INSERT INTO ollama_endpoints (id, name, base_url, kind, enabled, is_default, auth_token) \
                 VALUES ($1, $2, $3, $4, $5, $6, $7) \
                 RETURNING id, name, base_url, kind, enabled, is_default, auth_token",
                &[&id, &input.name, &input.base_url, &input.kind, &input.enabled, &input.is_default, &auth_token],
            ).await?
        };

        Ok(OllamaEndpointRecord {
            id: row.get("id"),
            name: row.get("name"),
            base_url: row.get("base_url"),
            kind: row.get("kind"),
            enabled: row.get("enabled"),
            is_default: row.get("is_default"),
            auth_token: row.get("auth_token"),
        })
    }

    pub async fn delete_ollama_endpoint(&self, id: &str) -> Result<(), BobError> {
        let client = self.pool.get().await?;
        client.execute("DELETE FROM ollama_endpoints WHERE id = $1", &[&id]).await?;
        Ok(())
    }

    pub async fn set_default_ollama_endpoint(&self, id: &str) -> Result<OllamaEndpointRecord, BobError> {
        let client = self.pool.get().await?;
        client.execute("UPDATE ollama_endpoints SET is_default = false", &[]).await?;
        let row = client.query_one(
            "UPDATE ollama_endpoints SET is_default = true WHERE id = $1 \
             RETURNING id, name, base_url, kind, enabled, is_default, auth_token",
            &[&id],
        ).await?;
        Ok(OllamaEndpointRecord {
            id: row.get("id"),
            name: row.get("name"),
            base_url: row.get("base_url"),
            kind: row.get("kind"),
            enabled: row.get("enabled"),
            is_default: row.get("is_default"),
            auth_token: row.get("auth_token"),
        })
    }

    pub async fn get_ollama_endpoint(&self, id: &str) -> Result<OllamaEndpointRecord, BobError> {
        let client = self.pool.get().await?;
        let row = client.query_one(
            "SELECT id, name, base_url, kind, enabled, is_default, auth_token \
             FROM ollama_endpoints WHERE id = $1",
            &[&id],
        ).await?;
        Ok(OllamaEndpointRecord {
            id: row.get("id"),
            name: row.get("name"),
            base_url: row.get("base_url"),
            kind: row.get("kind"),
            enabled: row.get("enabled"),
            is_default: row.get("is_default"),
            auth_token: row.get("auth_token"),
        })
    }

    pub async fn resolve_ollama_endpoint(&self, id: Option<&str>, fallback_url: &str) -> Result<OllamaEndpointRecord, BobError> {
        if let Some(id) = id {
            return self.get_ollama_endpoint(id).await;
        }

        let client = self.pool.get().await?;
        let rows = client.query(
            "SELECT id, name, base_url, kind, enabled, is_default, auth_token \
             FROM ollama_endpoints WHERE is_default = true LIMIT 1",
            &[],
        ).await?;

        if let Some(row) = rows.first() {
            Ok(OllamaEndpointRecord {
                id: row.get("id"),
                name: row.get("name"),
                base_url: row.get("base_url"),
                kind: row.get("kind"),
                enabled: row.get("enabled"),
                is_default: row.get("is_default"),
                auth_token: row.get("auth_token"),
            })
        } else {
            Ok(OllamaEndpointRecord {
                id: String::new(),
                name: "fallback".to_string(),
                base_url: fallback_url.to_string(),
                kind: "ollama".to_string(),
                enabled: true,
                is_default: false,
                auth_token: None,
            })
        }
    }
}
