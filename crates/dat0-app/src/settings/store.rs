use crate::settings::Settings;
use anyhow::{Context, Result};
use std::path::PathBuf;

pub struct SettingsStore {
    path: PathBuf,
}

impl SettingsStore {
    pub fn with_path(path: PathBuf) -> Self {
        Self { path }
    }

    pub fn load_or_default(&self) -> Result<Settings> {
        match std::fs::read_to_string(&self.path) {
            Ok(contents) => {
                let s: Settings = toml::from_str(&contents)
                    .with_context(|| format!("parse {}", self.path.display()))?;
                Ok(s)
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Settings::default()),
            Err(e) => Err(e).with_context(|| format!("read {}", self.path.display())),
        }
    }

    pub fn save(&self, s: &Settings) -> Result<()> {
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let serialized = toml::to_string_pretty(s)?;
        // Atomic write: write to .tmp, fsync, rename.
        let tmp = self.path.with_extension("toml.tmp");
        std::fs::write(&tmp, serialized)?;
        std::fs::rename(&tmp, &self.path)?;
        Ok(())
    }
}
