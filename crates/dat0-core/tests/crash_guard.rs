use dat0_core::boot::CrashGuard;
use dat0_core::telemetry::crash::prior_crash_detected;
use tempfile::tempdir;

#[test]
fn guard_marks_on_arm_and_clears_on_drop() {
    let dir = tempdir().unwrap();
    {
        let _g = CrashGuard::arm(dir.path()).unwrap();
        assert!(prior_crash_detected(dir.path()), "armed → marker present");
    } // drop = clean exit
    assert!(
        !prior_crash_detected(dir.path()),
        "dropped → marker cleared"
    );
}
