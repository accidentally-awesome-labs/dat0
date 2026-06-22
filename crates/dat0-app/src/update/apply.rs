//! Atomic self-update: writability probe, swap, rollback, relaunch.
//!
//! Platform sequences follow the T0 spike (docs/internal/2026-06-22-p10a-2-t0-spike.md).
//! Swap logic is inlined in `apply_update`; no standalone `plan_swap` is exposed
//! because T6 uses `apply_update` directly and a separate `plan_swap` would be
//! dead code with no callers.

use std::path::{Path, PathBuf};

/// Returns the install root for this binary.
///
/// - macOS: walks ancestors from `current_exe()` to find the `*.app` bundle.
///   The exe is at `dat0.app/Contents/MacOS/dat0`; `.nth(3)` from the exe
///   (nth(0)=exe, nth(1)=MacOS/, nth(2)=Contents/, nth(3)=dat0.app/) yields the bundle.
/// - Linux: `$APPIMAGE` env var (set by AppImage runtime), else `current_exe()`.
/// - Other platforms: `None`.
pub fn install_root() -> Option<PathBuf> {
    #[cfg(target_os = "macos")]
    {
        let exe = std::env::current_exe().ok()?;
        // exe is at dat0.app/Contents/MacOS/dat0
        let bundle = exe.ancestors().nth(3)?;
        if bundle.extension().is_some_and(|e| e == "app") {
            Some(bundle.to_path_buf())
        } else {
            None
        }
    }
    #[cfg(target_os = "linux")]
    {
        if let Ok(p) = std::env::var("APPIMAGE") {
            Some(PathBuf::from(p))
        } else {
            std::env::current_exe().ok()
        }
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        None
    }
}

/// Returns `true` if `path` is writable by the current user.
///
/// Probes by attempting to create and remove a temp file inside `path`.
/// For a `.app` bundle, `path` itself is a directory; the probe file is placed
/// directly inside it.
pub fn is_writable(path: &Path) -> bool {
    let probe = path.join(".dat0_writable_probe");
    match std::fs::write(&probe, b"") {
        Ok(_) => {
            let _ = std::fs::remove_file(&probe);
            true
        }
        Err(_) => false,
    }
}

/// Atomically swaps `downloaded` into `install`, with rollback on failure.
///
/// # macOS
/// 1. Extract the `.app.tar.gz` to a temp dir on the **same filesystem** as `install`
///    (same-filesystem is required for atomic `rename(2)`).
/// 2. Move the current bundle aside (backup).
/// 3. Move the new bundle into place.
/// 4. On any error in steps 2–3, restore the backup.
/// 5. Clean up on success.
///
/// IMPORTANT: extraction happens in Phase 1, **before** the install is touched, so a
/// missing or corrupt download fails early without disturbing the running app.
///
/// Does NOT relaunch; call [`relaunch`] after this returns `Ok`.
#[cfg(target_os = "macos")]
pub fn apply_update(install: &Path, downloaded: &Path) -> anyhow::Result<()> {
    use anyhow::Context;

    // --- Phase 1: validate + extract BEFORE touching the install ---
    // Use a sibling temp dir (same filesystem as install → rename is atomic).
    let install_parent = install.parent().context("install path has no parent")?;
    let tmp_extract = install_parent.join(".dat0_update_extract");
    if tmp_extract.exists() {
        std::fs::remove_dir_all(&tmp_extract).context("could not clean stale extract dir")?;
    }
    std::fs::create_dir_all(&tmp_extract).context("could not create extract dir")?;

    // Extract — fails here if `downloaded` doesn't exist or is not a valid tar.gz.
    let status = std::process::Command::new("tar")
        .arg("-xzf")
        .arg(downloaded)
        .arg("-C")
        .arg(&tmp_extract)
        .status()
        .context("failed to launch tar")?;
    if !status.success() {
        let _ = std::fs::remove_dir_all(&tmp_extract);
        anyhow::bail!("tar extraction failed (exit {:?})", status.code());
    }

    // Find the .app bundle inside the extract dir.
    let extracted_app =
        find_app_in_dir(&tmp_extract).context("no .app bundle found in extracted archive")?;

    // --- Phase 2: move install aside (backup) ---
    let backup_path = install_parent.join(".dat0_update_backup.app");
    if backup_path.exists() {
        std::fs::remove_dir_all(&backup_path).context("could not remove stale backup")?;
    }
    std::fs::rename(install, &backup_path).context("could not move install aside for backup")?;

    // --- Phase 3: move new bundle into place ---
    if let Err(e) = std::fs::rename(&extracted_app, install) {
        // Rollback: restore backup.
        if backup_path.exists() {
            if let Err(rollback_err) = std::fs::rename(&backup_path, install) {
                tracing::error!(
                    "update rollback failed; install may be missing at {}: {rollback_err}",
                    install.display()
                );
            }
        }
        let _ = std::fs::remove_dir_all(&tmp_extract);
        return Err(e).context("could not move new bundle into place");
    }

    // --- Phase 4: cleanup ---
    let _ = std::fs::remove_dir_all(&backup_path);
    let _ = std::fs::remove_dir_all(&tmp_extract);

    Ok(())
}

#[cfg(target_os = "macos")]
fn find_app_in_dir(dir: &Path) -> Option<PathBuf> {
    // The archive root entry is `dat0.app/`, so the extracted dir itself may be a .app,
    // or there may be a .app directly inside `dir`.
    if dir.extension().is_some_and(|e| e == "app") {
        return Some(dir.to_path_buf());
    }
    for entry in std::fs::read_dir(dir).ok()? {
        let entry = entry.ok()?;
        let path = entry.path();
        if path.is_dir() && path.extension().is_some_and(|e| e == "app") {
            return Some(path);
        }
    }
    None
}

/// Atomically swaps `downloaded` AppImage into `install`, with rollback on failure.
///
/// # Linux
/// 1. Validate that `downloaded` exists.
/// 2. `chmod +x` the downloaded file.
/// 3. Move the current AppImage aside as a backup.
/// 4. `rename` the downloaded file over the target (atomic, new inode — safe while running).
/// 5. On error in steps 3–4, restore the backup.
/// 6. Clean up backup on success.
///
/// Does NOT relaunch; call [`relaunch`] after this returns `Ok`.
#[cfg(target_os = "linux")]
pub fn apply_update(install: &Path, downloaded: &Path) -> anyhow::Result<()> {
    use anyhow::Context;
    use std::os::unix::fs::PermissionsExt;

    // --- Phase 1: validate BEFORE touching install ---
    if !downloaded.exists() {
        anyhow::bail!(
            "downloaded payload does not exist: {}",
            downloaded.display()
        );
    }

    // chmod +x
    let mut perms = std::fs::metadata(downloaded)
        .context("could not stat downloaded file")?
        .permissions();
    perms.set_mode(perms.mode() | 0o755);
    std::fs::set_permissions(downloaded, perms).context("could not chmod downloaded file")?;

    // --- Phase 2: backup ---
    let backup_path = {
        let mut p = install.to_path_buf();
        let fname = p
            .file_name()
            .map(|n| format!("{}.bak", n.to_string_lossy()))
            .unwrap_or_else(|| "dat0.bak".to_string());
        p.set_file_name(fname);
        p
    };
    std::fs::rename(install, &backup_path).context("could not move install aside for backup")?;

    // --- Phase 3: rename downloaded over target (MANDATORY — never cp/write-in-place) ---
    if let Err(e) = std::fs::rename(downloaded, install) {
        // Rollback
        if backup_path.exists() {
            if let Err(rollback_err) = std::fs::rename(&backup_path, install) {
                tracing::error!(
                    "update rollback failed; install may be missing at {}: {rollback_err}",
                    install.display()
                );
            }
        }
        return Err(e).context("could not rename new binary into place");
    }

    // --- Phase 4: cleanup backup ---
    let _ = std::fs::remove_file(&backup_path);

    Ok(())
}

/// Not supported on this platform.
#[cfg(not(any(target_os = "macos", target_os = "linux")))]
pub fn apply_update(_install: &Path, _downloaded: &Path) -> anyhow::Result<()> {
    anyhow::bail!("apply_update not supported on this platform")
}

/// Relaunches the app after a successful update. **Does not return on success.**
///
/// - macOS: `open dat0.app` (via Launch Services — respects entitlements + quarantine).
/// - Linux: `exec` into the new binary (replaces the current process image).
pub fn relaunch(install: &Path) -> ! {
    #[cfg(target_os = "macos")]
    {
        let _ = std::process::Command::new("open").arg(install).spawn();
        std::process::exit(0);
    }
    #[cfg(target_os = "linux")]
    {
        use std::os::unix::process::CommandExt;
        let err = std::process::Command::new(install).exec();
        eprintln!("relaunch failed: {err}");
        std::process::exit(1);
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        let _ = install;
        eprintln!("relaunch not supported on this platform");
        std::process::exit(1);
    }
}
