use anyhow::Result;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PolicyRule {
    pub kind: String,
    pub pattern: String,
    pub allowed: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PolicySyncSummary {
    pub scope: String,
    pub tool_rules: usize,
    pub path_rules: usize,
    pub command_rules: usize,
}

pub fn load_db_policy_rules(_postgres_url: &str, _scope: &str) -> Result<(Vec<PolicyRule>, PolicySyncSummary)> {
    todo!("load policy rules from postgres")
}
