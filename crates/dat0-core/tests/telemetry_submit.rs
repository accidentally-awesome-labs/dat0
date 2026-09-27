use dat0_core::telemetry::{self, crash::StagedCrash};

// With no telemetry initialized and reports off in the settings (their
// default), the submit helpers send nothing, say so, and count nothing.
//
// The settings are a directory of the test's own: the helpers read the
// opt-in when they are called (step 5.11a), and a developer's own settings
// with reports on would otherwise send these to the real endpoint.
#[test]
fn submit_helpers_send_nothing_while_reports_are_off() {
    let dir = tempfile::tempdir().expect("tempdir");
    // SAFETY: this binary holds one test, so no other thread reads the
    // variable while it is written.
    unsafe { std::env::set_var("DAT0_CONFIG_DIR", dir.path()) };
    assert!(!telemetry::submission_allowed(), "off by default");
    assert!(!telemetry::is_active(), "no client bound by default");

    let before = telemetry::egress::total_sent();
    assert_eq!(
        telemetry::submit_report("hello"),
        telemetry::Submission::Off
    );
    telemetry::submit_test_event("dat0 telemetry e2e 0.1.0"); // must not panic
    let crash = StagedCrash {
        message: "m".into(),
        backtrace: "b".into(),
        version: "0".into(),
    };
    assert_eq!(
        telemetry::submit_staged(&crash, Some("note")),
        telemetry::Submission::Off
    );
    assert!(
        !telemetry::is_active(),
        "and no client was started for them"
    );
    assert_eq!(telemetry::egress::total_sent(), before, "nothing counted");
}
