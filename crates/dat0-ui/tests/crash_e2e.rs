//! Out-of-process crash capture: spawn the real `dat0` binary with the hidden
//! debug-only `__crash-test` verb, let it panic, and assert the panic hook
//! staged a `last-crash.json` sentinel.
//!
//! This exercises the one crash seam no in-process test can reach:
//! `boot::CrashGuard::arm` -> real `std::panic` -> `install_panic_hook`'s
//! closure -> `write_staged`. `std::panic::set_hook` is process-global, so the
//! faithful test is a separate process.
//!
//! `CARGO_BIN_EXE_dat0` is injected by cargo for integration tests of this
//! package (bin name `dat0`). The child is built in the dev/test profile ->
//! `debug_assertions` on -> the `__crash-test` verb is present.

use std::process::Command;

use dat0_core::telemetry::crash;

#[test]
fn a_real_panic_stages_a_redacted_sentinel() {
    let dir = tempfile::tempdir().unwrap();
    // A SEPARATE tempdir contains the child's `AppContext::boot()` side effects
    // (config/data dirs, settings load + watcher thread, the sqlite-scanner
    // bootstrap DB) so the test never touches the real dev/CI dirs and boots
    // with deterministic default settings. (DuckDB's own extension cache at
    // `~/.duckdb/extensions/` is process-global and NOT redirected by these env
    // vars — but it is orthogonal to the crash sentinel.) `dir` (the argv) stays
    // the sole crash assertion target.
    let scratch = tempfile::tempdir().unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_dat0"))
        .arg("__crash-test")
        .arg(dir.path())
        .env("DAT0_CONFIG_DIR", scratch.path())
        .env("XDG_DATA_HOME", scratch.path())
        .output()
        .expect("spawn dat0 __crash-test");

    // Dev unwind -> exit 101; release abort -> SIGABRT. `!success()` covers both.
    assert!(
        !out.status.success(),
        "child must crash, got {:?}",
        out.status
    );

    // WRITE PATH: the real hook staged last-crash.json.
    let staged = crash::read_staged(dir.path())
        .expect("last-crash.json present + parseable after a real panic");

    // END-TO-END REDACTION through the real binary + real hook: the marker
    // survives, the fake-PII path does not.
    assert!(
        staged.message.contains("dat0 __crash-test sentinel"),
        "panic marker preserved: {}",
        staged.message
    );
    assert!(
        !staged.message.contains("/Users/secretuser"),
        "absolute path must be redacted end-to-end: {}",
        staged.message
    );
    assert!(!staged.backtrace.is_empty(), "backtrace captured");
    assert_eq!(
        staged.version,
        dat0_core::about::build_info::BuildInfo::current().version,
        "the sentinel names the build that produced it"
    );

    // The marker survived the abnormal exit (`mem::forget` -> no clear_running)
    // — the exact precondition the seeded-sentinel relaunch prompt assumes.
    assert!(
        crash::prior_crash_detected(dir.path()),
        "running.marker survives an abnormal exit"
    );
}
