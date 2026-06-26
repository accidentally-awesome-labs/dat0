use dat0_app::telemetry::crash::{
    self, StagedCrash, clear_running, clear_staged, mark_running, prior_crash_detected,
    read_staged, write_staged,
};
use tempfile::tempdir;

#[test]
fn marker_lifecycle_detects_unclean_exit() {
    let dir = tempdir().unwrap();
    assert!(
        !prior_crash_detected(dir.path()),
        "no marker on a clean dir"
    );
    mark_running(dir.path()).unwrap();
    assert!(
        prior_crash_detected(dir.path()),
        "marker present after mark_running"
    );
    clear_running(dir.path());
    assert!(
        !prior_crash_detected(dir.path()),
        "marker gone after clear_running"
    );
}

#[test]
fn staged_crash_round_trips_and_clears() {
    let dir = tempdir().unwrap();
    assert!(read_staged(dir.path()).is_none());
    let crash = StagedCrash {
        message: "panicked at 'boom'".into(),
        backtrace: "frame0\nframe1".into(),
        version: "0.1.0".into(),
    };
    write_staged(dir.path(), &crash).unwrap();
    assert_eq!(read_staged(dir.path()).as_ref(), Some(&crash));
    clear_staged(dir.path());
    assert!(read_staged(dir.path()).is_none());
}

#[test]
fn read_staged_is_none_on_corrupt_json() {
    let dir = tempdir().unwrap();
    std::fs::write(crash::staged_path(dir.path()), b"{not json").unwrap();
    assert!(
        read_staged(dir.path()).is_none(),
        "corrupt staging must not panic"
    );
}

#[test]
fn payload_from_panic_redacts_paths_and_keeps_message() {
    let payload = std::panic::catch_unwind(|| {
        // build a PanicHookInfo indirectly: set a hook that captures it.
        let captured = std::sync::Arc::new(std::sync::Mutex::new(None));
        let c2 = captured.clone();
        std::panic::set_hook(Box::new(move |info| {
            *c2.lock().unwrap() = Some(dat0_app::telemetry::crash::payload_from_panic(
                info, "9.9.9",
            ));
        }));
        let _ = std::panic::catch_unwind(|| panic!("boom at /Users/alice/secret/x.rs"));
        let _ = std::panic::take_hook();
        captured.lock().unwrap().clone()
    })
    .unwrap()
    .expect("hook captured a payload");

    assert_eq!(payload.version, "9.9.9");
    assert!(payload.message.contains("boom"), "keeps panic message");
    assert!(
        !payload.message.contains("/Users/alice"),
        "absolute path must be redacted from staged message: {}",
        payload.message
    );
}
