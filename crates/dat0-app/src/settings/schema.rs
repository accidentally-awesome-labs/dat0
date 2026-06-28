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
    /// Whether first-run onboarding (enriched hero + auto-tour) has been
    /// completed or skipped. Per-install, not per-workspace. Absent in
    /// pre-v2 settings.toml → false, so upgraders see the tour once.
    #[serde(default)]
    pub first_run_done: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            schema_version: 2,
            profile: Profile::default(),
            theme: Theme::default(),
            telemetry: Telemetry::default(),
            workspace: Workspace::default(),
            ai: AiSettings::default(),
            update_auto_check: true,
            memory_budget_mb: 1024,
            log_level: "info,dat0=debug".into(),
            first_run_done: false,
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

/// Persist the first-run-onboarding-complete flag via the atomic write path.
pub fn set_first_run_done(
    store: &crate::settings::store::SettingsStore,
    done: bool,
) -> anyhow::Result<()> {
    let mut s = store.load_or_default()?;
    s.first_run_done = done;
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

#[cfg(test)]
mod first_run_tests {
    use super::*;
    use crate::settings::store::SettingsStore;

    #[test]
    fn first_run_done_defaults_false() {
        // Fresh Settings (no file) => onboarding pending.
        assert!(!Settings::default().first_run_done);
    }

    #[test]
    fn absent_field_in_old_toml_reads_false() {
        // A pre-v2 settings.toml has no `first_run_done` key.
        let toml = "schema_version = 1\n";
        let s: Settings = toml::from_str(toml).unwrap();
        assert!(
            !s.first_run_done,
            "absent field must read as false (upgrader sees tour once)"
        );
    }

    #[test]
    fn set_first_run_done_round_trips() {
        let store = SettingsStore::open_in_memory();
        set_first_run_done(&store, true).unwrap();
        assert!(store.load_or_default().unwrap().first_run_done);
        set_first_run_done(&store, false).unwrap();
        assert!(!store.load_or_default().unwrap().first_run_done);
    }

    #[test]
    fn default_schema_version_is_2() {
        assert_eq!(Settings::default().schema_version, 2);
    }
}
