use crate::settings::Settings;
use anyhow::{Context, Result};
use std::io::Write as _;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

/// Best-effort RAII guard for the per-store tempdir created by
/// [`SettingsStore::open_in_memory`]. On `Drop` we remove the directory
/// tree if it still exists; errors are swallowed because the OS will
/// reclaim `/tmp` eventually and a partial cleanup must never block
/// test teardown. Kept in-crate to avoid a new dependency on `tempfile`
/// — the existing P1 tests use `tempfile::tempdir()` via `dev-deps`,
/// but `open_in_memory` is part of the runtime crate's public API and
/// must work in non-test builds too (T11 deferral close in
/// `docs/deferrals.md`).
struct InMemoryGuard(PathBuf);

impl Drop for InMemoryGuard {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

pub struct SettingsStore {
    path: PathBuf,
    // Holds the tempdir guard for in-memory stores so the backing file
    // survives until the store is dropped. `None` for on-disk stores.
    _tempdir: Option<InMemoryGuard>,
}

impl SettingsStore {
    pub fn with_path(path: PathBuf) -> Self {
        Self {
            path,
            _tempdir: None,
        }
    }

    /// Open a transient `SettingsStore` backed by a per-process unique
    /// directory under `std::env::temp_dir()`. Used by integration tests
    /// (`tests/settings_ui.rs`) and by any caller that wants a throwaway
    /// settings store without polluting the user's config dir. The
    /// directory is removed when the store is dropped.
    pub fn open_in_memory() -> Self {
        // Per-process counter keeps multiple in-memory stores in a single
        // test binary from colliding on the same path. PID is included
        // for cargo-test's process isolation.
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("dat0-settings-{}-{}", std::process::id(), n));
        // SAFETY: panicking here is acceptable for a test/utility helper
        // — there is no recovery path if the OS can't provide a temp dir.
        std::fs::create_dir_all(&dir).expect("create tempdir for in-memory SettingsStore");
        let path = dir.join("settings.toml");
        Self {
            path,
            _tempdir: Some(InMemoryGuard(dir)),
        }
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

    /// Atomically persist `s` to `settings.toml`.
    ///
    /// Writes to `settings.toml.tmp` first, syncs the file, then renames to
    /// `settings.toml`, then syncs the parent directory so the rename metadata
    /// also reaches stable storage (PD-002 — same pattern as
    /// `session/mod.rs::persist` which closed the session.json twin in P4a T8).
    /// The `.tmp` file is never visible after a successful call.
    pub fn save(&self, s: &Settings) -> Result<()> {
        let parent = self.path.parent();
        if let Some(p) = parent {
            std::fs::create_dir_all(p)?;
        }
        let serialized =
            toml::to_string_pretty(s).context("settings::save: TOML serialisation failed")?;

        let tmp = self.path.with_extension("toml.tmp");

        {
            let f = std::fs::OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(true)
                .open(&tmp)
                .with_context(|| {
                    format!("settings::save: create tmp file {} failed", tmp.display())
                })?;
            let mut bw = std::io::BufWriter::new(f);
            bw.write_all(serialized.as_bytes())
                .context("settings::save: write to tmp failed")?;
            let f = bw
                .into_inner()
                .context("settings::save: flush BufWriter failed")?;
            f.sync_all()
                .context("settings::save: fsync tmp file failed")?;
        }

        std::fs::rename(&tmp, &self.path).with_context(|| {
            format!(
                "settings::save: rename {} -> {} failed",
                tmp.display(),
                self.path.display()
            )
        })?;

        // fsync the parent directory so the rename metadata hits disk.
        // PD-002: without this, a power-loss between the rename and any
        // future OS-triggered directory sync could lose the new file.
        if let Some(p) = parent {
            let dir = std::fs::File::open(p).with_context(|| {
                format!("settings::save: open parent dir {} failed", p.display())
            })?;
            dir.sync_all()
                .context("settings::save: fsync parent dir failed")?;
        }

        Ok(())
    }

    /// KV-style getter mapping logical dotted keys to the underlying
    /// [`Settings`] struct fields. Returns `None` if the key is unknown
    /// or the stored value is empty. P3b T11 introduces this facade so
    /// settings-UI widgets (Profile name/email, Theme dropdown) and T12's
    /// `Theme::switch` can be wired against a stable string-keyed API
    /// without coupling to the TOML schema field names.
    ///
    /// Supported keys:
    /// - `author.name` → `Settings::profile.author_name`
    /// - `author.email` → `Settings::profile.author_email`
    /// - `theme.id` → `Settings::theme.name` (the logical "id" the user
    ///   picks in the dropdown; T12 uses this exact key for live-switch)
    ///
    /// Unknown keys return `None`; on I/O or parse errors, the store
    /// degrades to `None` and logs at `warn` so a corrupt settings file
    /// doesn't crash the running app.
    pub fn get_string(&self, key: &str) -> Option<String> {
        let settings = match self.load_or_default() {
            Ok(s) => s,
            Err(e) => {
                tracing::warn!(?e, key, "SettingsStore::get_string load failed");
                return None;
            }
        };
        let value = match key {
            "author.name" => settings.profile.author_name,
            "author.email" => settings.profile.author_email,
            "theme.id" => settings.theme.name,
            _ => return None,
        };
        if value.is_empty() { None } else { Some(value) }
    }

    /// KV-style setter — the write-side counterpart of [`get_string`].
    /// Loads the current [`Settings`], updates the field selected by
    /// `key`, and persists via the same atomic write path used by
    /// [`save`]. Returns an error if the key is unknown or the write
    /// fails (callers in the UI layer should surface this through the
    /// banner system; tests can `.unwrap()`).
    pub fn set(&self, key: &str, value: &str) -> Result<()> {
        let mut settings = self.load_or_default()?;
        match key {
            "author.name" => settings.profile.author_name = value.to_string(),
            "author.email" => settings.profile.author_email = value.to_string(),
            "theme.id" => settings.theme.name = value.to_string(),
            _ => anyhow::bail!("SettingsStore::set: unknown key {key}"),
        }
        self.save(&settings)
    }
}
