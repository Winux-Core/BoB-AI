use std::path::Path;
use anyhow::Result;
use serde::{Deserialize, Serialize};
use tracing::info;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MigrationSummary {
    pub applied: usize,
    pub skipped: usize,
    pub total: usize,
}

const MIGRATIONS: &[(&str, &str)] = &[
    ("001_init", r#"
CREATE TABLE IF NOT EXISTS conversations (
    id TEXT PRIMARY KEY,
    title TEXT NOT NULL DEFAULT 'New Conversation',
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE IF NOT EXISTS messages (
    id TEXT PRIMARY KEY,
    conversation_id TEXT NOT NULL REFERENCES conversations(id) ON DELETE CASCADE,
    role TEXT NOT NULL,
    content TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE IF NOT EXISTS workspace_profile (
    id TEXT PRIMARY KEY DEFAULT 'default',
    default_model TEXT NOT NULL DEFAULT 'llama3.1',
    system_prompt TEXT NOT NULL DEFAULT '',
    context_injection TEXT NOT NULL DEFAULT '',
    personalization JSONB NOT NULL DEFAULT '{}'
);

CREATE TABLE IF NOT EXISTS ollama_endpoints (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    base_url TEXT NOT NULL,
    kind TEXT NOT NULL DEFAULT 'local',
    enabled BOOLEAN NOT NULL DEFAULT TRUE,
    is_default BOOLEAN NOT NULL DEFAULT FALSE,
    auth_token TEXT
);

CREATE INDEX IF NOT EXISTS idx_messages_conversation ON messages(conversation_id, created_at);
CREATE INDEX IF NOT EXISTS idx_conversations_updated ON conversations(updated_at DESC);
"#),
    ("002_policy_rules", r#"
CREATE TABLE IF NOT EXISTS policy_rules (
    id TEXT PRIMARY KEY,
    scope TEXT NOT NULL DEFAULT 'default',
    kind TEXT NOT NULL CHECK (kind IN ('tool', 'path', 'command')),
    pattern TEXT NOT NULL,
    allowed BOOLEAN NOT NULL DEFAULT TRUE,
    priority INTEGER NOT NULL DEFAULT 0,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_policy_rules_scope ON policy_rules(scope);
CREATE INDEX IF NOT EXISTS idx_policy_rules_scope_kind ON policy_rules(scope, kind);
"#),
];

pub fn apply_migrations(postgres_url: &str, _dir: &Path) -> Result<MigrationSummary> {
    let mut client = postgres::Client::connect(postgres_url, postgres::NoTls)?;

    client.batch_execute(
        "CREATE TABLE IF NOT EXISTS _migrations (
            name TEXT PRIMARY KEY,
            applied_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
        );"
    )?;

    let total = MIGRATIONS.len();
    let mut applied = 0;
    let mut skipped = 0;

    for (name, sql) in MIGRATIONS {
        let row = client.query_one(
            "SELECT COUNT(*) AS c FROM _migrations WHERE name = $1",
            &[name],
        )?;
        let count: i64 = row.get("c");

        if count > 0 {
            skipped += 1;
            continue;
        }

        info!(migration = name, "applying migration");
        let mut tx = client.transaction()?;
        tx.batch_execute(sql)?;
        tx.execute("INSERT INTO _migrations (name) VALUES ($1)", &[name])?;
        tx.commit()?;
        applied += 1;
    }

    Ok(MigrationSummary { applied, skipped, total })
}
