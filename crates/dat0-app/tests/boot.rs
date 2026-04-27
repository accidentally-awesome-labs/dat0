//! Boot-orchestration integration test.
//!
//! Note: this test runs `AppContext::boot()` against the real user config
//! directory (e.g. `~/Library/Application Support/dat0/`). The P1 plan
//! acknowledges this side effect on the host and chooses simplicity over
//! isolation. Marked `#[serial]` to avoid interleaving with sibling tests
//! that may also touch the same paths.

use dat0_app::boot::AppContext;
use serial_test::serial;

#[test]
#[serial]
fn boot_returns_ok_with_defaults() {
    let ctx = AppContext::boot();
    assert!(ctx.is_ok(), "boot failed: {:?}", ctx.err());
    let ctx = ctx.unwrap();
    let snapshot = ctx.settings.read().unwrap().clone();
    assert_eq!(snapshot.theme.name, "dark");
    assert!(!snapshot.telemetry.crash_submission_enabled);
}
