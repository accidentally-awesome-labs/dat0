//! Boot-orchestration integration test.
//!
//! Isolated with `DAT0_CONFIG_DIR`, the same relocation seam the portable
//! install uses. It did not used to be: the original ran `AppContext::boot()`
//! against the real user config directory and chose simplicity over isolation,
//! which meant it asserted the developer's own `settings.toml` and passed or
//! failed by accident. S9 exposed that — flipping the default theme to light
//! failed the test on any machine that had ever launched dat0 and picked dark.
//!
//! `#[serial]` because the override is a process-global environment variable.

use std::path::Path;

use dat0_core::boot::AppContext;
use serial_test::serial;

#[test]
#[serial]
fn boot_returns_ok_with_defaults() {
    let dir = tempfile::tempdir().expect("temp config dir");
    with_config_dir(dir.path(), || {
        let ctx = AppContext::boot();
        assert!(ctx.is_ok(), "boot failed: {:?}", ctx.err());
        let ctx = ctx.unwrap();
        let snapshot = ctx.settings.read().unwrap().clone();
        assert_eq!(
            snapshot.theme.name,
            dat0_core::theme::DEFAULT_ID,
            "a fresh install must boot the default builtin"
        );
        assert!(!snapshot.telemetry.crash_submission_enabled);
    });
}

/// Run `f` with `DAT0_CONFIG_DIR` pointed at `dir`, restoring it afterwards.
///
/// Restores rather than unsets: a caller that had one set — CI, or a developer
/// running against a scratch profile — must get it back even if `f` panics is
/// not something this can promise, so the assertions live inside and the
/// restore is the last statement. `#[serial]` is what makes that safe.
fn with_config_dir(dir: &Path, f: impl FnOnce()) {
    let prev = std::env::var_os("DAT0_CONFIG_DIR");
    // SAFETY: single-threaded within this `#[serial]` test.
    unsafe { std::env::set_var("DAT0_CONFIG_DIR", dir) };
    f();
    match prev {
        Some(p) => unsafe { std::env::set_var("DAT0_CONFIG_DIR", p) },
        None => unsafe { std::env::remove_var("DAT0_CONFIG_DIR") },
    }
}
