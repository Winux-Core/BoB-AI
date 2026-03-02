use std::path::PathBuf;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BobConfig {
    pub allowed_roots: Vec<PathBuf>,
    pub permission_policy_path: PathBuf,
    pub audit_log_path: PathBuf,
    pub policy_sync_from_db: bool,
    pub postgres_url: String,
    pub ollama_url: String,
    pub fs_cache_path: PathBuf,
    pub migrations_dir: PathBuf,
    pub policy_scope: String,
}

impl BobConfig {
    pub fn from_env() -> Self {
        let allowed_roots = std::env::var("BOB_ALLOWED_ROOTS")
            .unwrap_or_default()
            .split(':')
            .filter(|s| !s.is_empty())
            .map(PathBuf::from)
            .collect();

        Self {
            allowed_roots,
            permission_policy_path: env_path("BOB_PERMISSION_POLICY_PATH", "bob-policy.json"),
            audit_log_path: env_path("BOB_AUDIT_LOG_PATH", "bob-audit.log"),
            policy_sync_from_db: std::env::var("BOB_POLICY_SYNC_FROM_DB")
                .map(|v| v == "true" || v == "1")
                .unwrap_or(false),
            postgres_url: std::env::var("BOB_POSTGRES_URL").unwrap_or_default(),
            ollama_url: std::env::var("BOB_OLLAMA_URL")
                .unwrap_or_else(|_| "http://127.0.0.1:11434".to_string()),
            fs_cache_path: env_path("BOB_FS_CACHE_PATH", "bob-fs-cache.bin"),
            migrations_dir: env_path("BOB_MIGRATIONS_DIR", "migrations"),
            policy_scope: std::env::var("BOB_POLICY_SCOPE")
                .unwrap_or_else(|_| "default".to_string()),
        }
    }
}

fn env_path(var: &str, default: &str) -> PathBuf {
    std::env::var(var)
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from(default))
}
