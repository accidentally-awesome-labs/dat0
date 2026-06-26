use dat0_app::telemetry::{self, crash::StagedCrash};

// With no telemetry initialized (opt-in default off → no client bound), the
// submit helpers must be safe no-ops and is_active() must be false.
#[test]
fn submit_helpers_are_noops_when_inactive() {
    assert!(!telemetry::is_active(), "no client bound by default");
    telemetry::submit_report("hello"); // must not panic
    let crash = StagedCrash {
        message: "m".into(),
        backtrace: "b".into(),
        version: "0".into(),
    };
    telemetry::submit_staged(&crash, Some("note")); // must not panic
}
