use crate::ai::settings::AiSettings;
use serde::{Deserialize, Serialize};

fn default_true() -> bool {
    true
}

fn default_memory_budget_mb() -> u32 {
    1024
}

fn default_log_level() -> String {
    "info,dat0=debug".to_string()
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub schema_version: u32,
    pub profile: Profile,
    pub theme: Theme,
    pub telemetry: Telemetry,
    pub workspace: Workspace,
    pub ai: AiSettings,
    /// Whether to automatically check for updates at launch (default: true).
    /// Users can opt out in Settings → Updates.
    #[serde(default = "default_true")]
    pub update_auto_check: bool,
    /// Per-window DuckDB memory budget in MB (default 1024 = 1 GiB).
    /// Applies to windows opened after the change (PRAGMA memory_limit at engine init).
    #[serde(default = "default_memory_budget_mb")]
    pub memory_budget_mb: u32,
    /// tracing EnvFilter directive applied at next launch (default "info,dat0=debug").
    #[serde(default = "default_log_level")]
    pub log_level: String,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            schema_version: 1,
            profile: Profile::default(),
            theme: Theme::default(),
            telemetry: Telemetry::default(),
            workspace: Workspace::default(),
            ai: AiSettings::default(),
            update_auto_check: true,
            memory_budget_mb: 1024,
            log_level: "info,dat0=debug".into(),
        }
    }
}

/// Persist the per-window memory budget (MB) via the atomic settings write path.
pub fn set_memory_budget_mb(
    store: &crate::settings::store::SettingsStore,
    mb: u32,
) -> anyhow::Result<()> {
    let mut s = store.load_or_default()?;
    s.memory_budget_mb = mb;
    store.save(&s)
}

/// Persist the tracing log-level directive.
pub fn set_log_level(
    store: &crate::settings::store::SettingsStore,
    level: &str,
) -> anyhow::Result<()> {
    let mut s = store.load_or_default()?;
    s.log_level = level.to_string();
    store.save(&s)
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct Profile {
    pub author_name: String,
    pub author_email: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Theme {
    pub name: String,
}
impl Default for Theme {
    fn default() -> Self {
        Self {
            name: "dark".into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct Telemetry {
    pub crash_submission_enabled: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct Workspace {
    /// Paths always treated as networked; matched by prefix (design D2).
    pub treat_paths_as_networked: Vec<std::path::PathBuf>,
    /// Global override: treat every workspace as networked (design D2).
    pub treat_all_as_networked: bool,
}
