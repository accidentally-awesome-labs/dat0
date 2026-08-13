use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

const MAX_ENTRIES: usize = 25;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind")]
pub enum RecentEntry {
    Workspace { path: PathBuf },
    Package { path: PathBuf },
}

impl RecentEntry {
    pub fn path(&self) -> &std::path::Path {
        match self {
            RecentEntry::Workspace { path } | RecentEntry::Package { path } => path,
        }
    }
}

pub struct Recents {
    path: PathBuf,
    entries: Vec<RecentEntry>,
}

impl Recents {
    pub fn with_path(path: PathBuf) -> Self {
        let entries = std::fs::read_to_string(&path)
            .ok()
            .and_then(|s| serde_json::from_str::<Vec<RecentEntry>>(&s).ok())
            .unwrap_or_default();
        Self { path, entries }
    }

    pub fn list(&self) -> &[RecentEntry] {
        &self.entries
    }

    pub fn push(&mut self, entry: RecentEntry) -> Result<()> {
        self.entries.retain(|e| e.path() != entry.path());
        self.entries.insert(0, entry);
        self.entries.truncate(MAX_ENTRIES);
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let serialized = serde_json::to_string_pretty(&self.entries)?;
        std::fs::write(&self.path, serialized)?;
        Ok(())
    }
}
