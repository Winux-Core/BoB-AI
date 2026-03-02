use std::path::Path;
use anyhow::Result;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WatchSummary {
    pub events_processed: u64,
    pub entries_updated: u64,
    pub duration_ms: u64,
}

pub fn watch_and_persist(
    _root: &Path,
    _output: &Path,
    _exclude: &[String],
    _max_events: Option<u64>,
    _idle_timeout_ms: u64,
) -> Result<WatchSummary> {
    todo!("watch filesystem and persist changes to cache")
}
