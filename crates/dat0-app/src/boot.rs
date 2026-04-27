use anyhow::Result;
use std::sync::Arc;
use tracing_subscriber::{EnvFilter, fmt, prelude::*};

use crate::{
    platform,
    recents::Recents,
    settings::{Settings, store::SettingsStore, watcher::SettingsWatcher},
    telemetry::Telemetry,
};

/// Initialize the tracing subscriber. Idempotent — calling twice is a no-op.
pub fn init_logging() -> Result<()> {
    let filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info,dat0=debug"));
    let _ = tracing_subscriber::registry()
        .with(filter)
        .with(fmt::layer().with_target(false).compact())
        .try_init();
    Ok(())
}

/// Application context produced by [`AppContext::boot`]. Owns the long-lived
/// process-wide state: the live settings snapshot, the file watcher that keeps
/// it in sync, the recents store, and the telemetry guard.
pub struct AppContext {
    pub settings: Arc<std::sync::RwLock<Settings>>,
    pub _settings_watcher: SettingsWatcher,
    pub recents: Arc<std::sync::Mutex<Recents>>,
    pub _telemetry: Telemetry,
}

impl AppContext {
    /// Boot the application: ensure config + data dirs exist, load settings,
    /// start the settings watcher, open the recents store, and initialize
    /// telemetry per the user's opt-in preference.
    pub fn boot() -> Result<Self> {
        let cfg_dir = platform::config_dir()?;
        platform::ensure_dir(&cfg_dir)?;
        let data_dir = platform::data_dir()?;
        platform::ensure_dir(&data_dir)?;

        let settings_path = cfg_dir.join("settings.toml");
        let store = SettingsStore::with_path(settings_path.clone());
        let initial = store.load_or_default()?;
        // The watcher backs onto `notify`, which requires the path to exist
        // before `watch()` is called. On a fresh install `load_or_default`
        // returns defaults without touching disk, so seed the file once here.
        if !settings_path.exists() {
            store.save(&initial)?;
        }

        let settings = Arc::new(std::sync::RwLock::new(initial.clone()));
        let settings_clone = settings.clone();
        let watcher = SettingsWatcher::start(settings_path, move |new_settings| {
            *settings_clone.write().unwrap() = new_settings;
        })?;

        let recents = Arc::new(std::sync::Mutex::new(Recents::with_path(
            cfg_dir.join("recents.json"),
        )));

        let telemetry = Telemetry::init(initial.telemetry.crash_submission_enabled)?;

        Ok(Self {
            settings,
            _settings_watcher: watcher,
            recents,
            _telemetry: telemetry,
        })
    }
}
