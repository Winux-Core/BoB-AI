use std::path::Path;
use anyhow::Result;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MigrationSummary {
    pub applied: usize,
    pub skipped: usize,
    pub total: usize,
}

pub fn apply_migrations(_postgres_url: &str, _dir: &Path) -> Result<MigrationSummary> {
    todo!("apply SQL migrations from directory to postgres")
}
