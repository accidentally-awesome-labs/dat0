use anyhow::Result;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tracing_subscriber::{EnvFilter, fmt, prelude::*};

use crate::{
    error_ux::banner::{self, Banner},
    platform,
    recents::Recents,
    settings::{Settings, store::SettingsStore, watcher::SettingsWatcher},
    telemetry::Telemetry,
};

/// Initialize the tracing subscriber. Idempotent — calling twice is a no-op.
///
/// When `RUST_LOG` is unset, the fallback directive is read from the persisted
/// `settings.toml` (`log_level` field, default `"info,dat0=debug"`). On any
/// error reading the file the hardcoded default is used — never panics.
pub fn init_logging() -> Result<()> {
    let persisted = crate::settings::store::SettingsStore::with_path(
        platform::config_dir()
            .unwrap_or_default()
            .join("settings.toml"),
    )
    .load_or_default()
    .map(|s| s.log_level)
    .unwrap_or_else(|_| "info,dat0=debug".to_string());
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(&persisted));
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

        // Install the DuckDB sqlite_scanner extension exactly once before any
        // window opens. Engine init() in P2 only LOADs (not INSTALLs) on the
        // assumption this has already run — this avoids a multi-window race
        // for the shared `~/.duckdb/extensions/` cache (spec §2.5).
        install_sqlite_scanner(&data_dir);

        Ok(Self {
            settings,
            _settings_watcher: watcher,
            recents,
            _telemetry: telemetry,
        })
    }
}

/// RAII guard: marks the run "in progress" + installs the crash-staging panic
/// hook on `arm`, and removes the running marker on `Drop` (clean shutdown).
/// A marker that survives into the next launch signals an unclean exit.
pub struct CrashGuard {
    data_dir: PathBuf,
}

impl CrashGuard {
    pub fn arm(data_dir: &Path) -> std::io::Result<Self> {
        crate::telemetry::crash::mark_running(data_dir)?;
        // The panic hook stages `last-crash.json` unconditionally (panic context
        // must not read settings). The payload is local-only and redacted. On the
        // NEXT launch, if the user has opted OUT, `last-crash.json` is silently
        // discarded (no dialog, no submission). The opt-out guarantee is satisfied
        // by that relaunch discard, not by gating the hook itself.
        crate::telemetry::crash::install_panic_hook(data_dir.to_path_buf());
        Ok(Self {
            data_dir: data_dir.to_path_buf(),
        })
    }
}

impl Drop for CrashGuard {
    fn drop(&mut self) {
        crate::telemetry::crash::clear_running(&self.data_dir);
    }
}

/// Single-shot install of `sqlite_scanner` at app boot. Failures are logged
/// and surfaced as a pending [`Banner`] for the render layer to drain — the
/// app continues launching with SQLite ATTACH degraded.
fn install_sqlite_scanner(data_dir: &std::path::Path) {
    let scratch_dir = data_dir.join("ext-bootstrap");
    if let Err(e) = std::fs::create_dir_all(&scratch_dir) {
        tracing::warn!(
            error = %e,
            path = %scratch_dir.display(),
            "could not create extension bootstrap scratch dir; falling back to temp_dir"
        );
    }
    // If create_dir_all on data_dir failed, fall back to temp_dir so the
    // INSTALL still has a writable scratch DB to open against.
    let bootstrap_db = if scratch_dir.exists() {
        scratch_dir.join("bootstrap.duckdb")
    } else {
        std::env::temp_dir()
            .join("dat0")
            .join("ext-bootstrap")
            .join("bootstrap.duckdb")
    };
    if let Some(parent) = bootstrap_db.parent() {
        let _ = std::fs::create_dir_all(parent);
    }

    if let Err(e) =
        dat0_engine::extension_bootstrap::install_sqlite_scanner_at_app_boot(bootstrap_db)
    {
        tracing::error!(
            error = %e,
            "sqlite_scanner install failed at boot; SQLite ATTACH will be unavailable"
        );
        // Surface a Banner via the P1 error_ux primitive so the render layer
        // can drain and display it once notification surfaces are wired up.
        let title = dat0_i18n::t("boot.sqlite_scanner_install_failed.title");
        let body = dat0_i18n::t("boot.sqlite_scanner_install_failed.body");
        banner::push(Banner::warning_with_body(title, body));
    }
}
