//! Out-of-process crash e2e (Slice 7): spawn the real `dat0` binary with the
//! hidden debug-only `__crash-test` verb, let it panic, and assert the panic
//! hook staged a `last-crash.json` sentinel. This exercises the ONE crash seam
//! no in-process test can reach: `boot::CrashGuard::arm` → real `std::panic` →
//! `install_panic_hook`'s closure → `write_staged`. `std::panic::set_hook` is
//! process-global, so the faithful test is a separate process.
//!
//! `CARGO_BIN_EXE_dat0` is injected by cargo for integration tests of the
//! `dat0-app` package (bin name `dat0`). The child is built in the dev/test
//! profile → `debug_assertions` on → the `__crash-test` verb is present.

use std::process::Command;

use dat0_app::telemetry::crash;

#[test]
fn real_panic_stages_redacted_sentinel() {
    let dir = tempfile::tempdir().unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_dat0"))
        .arg("__crash-test")
        .arg(dir.path())
        .output()
        .expect("spawn dat0 __crash-test");

    // Dev unwind → exit 101; release abort → SIGABRT. `!success()` covers both.
    assert!(
        !out.status.success(),
        "child must crash, got {:?}",
        out.status
    );

    // WRITE PATH: the real hook staged last-crash.json.
    assert!(
        crash::read_staged(dir.path()).is_some(),
        "last-crash.json must be present + parseable after a real panic"
    );
}
