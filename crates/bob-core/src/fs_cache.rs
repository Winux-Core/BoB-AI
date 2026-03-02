use std::path::Path;
use anyhow::Result;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileEntry {
    pub path: String,
    pub size: u64,
    pub modified: u64,
}

pub struct FsIndex {
    entries: Vec<FileEntry>,
}

impl FsIndex {
    pub fn build(_root: &Path, _exclude: &[String]) -> Result<Self> {
        todo!("build filesystem index from root directory")
    }

    pub fn load(_path: &Path) -> Result<Self> {
        todo!("load filesystem index from cache file")
    }

    pub fn save(&self, _path: &Path) -> Result<()> {
        todo!("save filesystem index to cache file")
    }

    pub fn total_entries(&self) -> usize {
        self.entries.len()
    }

    pub fn lookup(&self, _path: &str) -> Option<&FileEntry> {
        todo!("lookup a file entry by path")
    }

    pub fn apply_path_change(&mut self, _path: &Path) -> Result<()> {
        todo!("apply a single path change to the index")
    }
}
