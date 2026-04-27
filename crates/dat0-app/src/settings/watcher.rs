use std::path::PathBuf;
use anyhow::Result;
use notify::{Watcher, RecursiveMode, RecommendedWatcher, EventKind};
use crate::settings::{Settings, store::SettingsStore};

pub struct SettingsWatcher {
    _watcher: RecommendedWatcher,
}

impl SettingsWatcher {
    pub fn start<F>(path: PathBuf, on_change: F) -> Result<Self>
    where
        F: Fn(Settings) + Send + 'static,
    {
        let watch_path = path.clone();
        let store = SettingsStore::with_path(path);
        let mut watcher = notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
            match res {
                Ok(event) => {
                    if matches!(event.kind, EventKind::Modify(_) | EventKind::Create(_)) {
                        if let Ok(s) = store.load_or_default() {
                            on_change(s);
                        }
                    }
                }
                Err(e) => tracing::warn!(?e, "settings watcher error"),
            }
        })?;
        watcher.watch(&watch_path, RecursiveMode::NonRecursive)?;
        Ok(Self { _watcher: watcher })
    }
}
