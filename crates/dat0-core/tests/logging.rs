//! The log reaches a file (step 5.11f).
//!
//! `init_logging` wrote to stdout alone, which a window opened from the
//! desktop has nowhere to show: dat0 kept no log, while `docs/privacy.md`
//! described one and Settings opened a folder with none in it. One test here
//! sets the process-wide subscriber, which is set once; the other uses the
//! file alone.

#[test]
fn init_logging_writes_the_log_to_a_file_in_the_cache_dir() {
    let dir = tempfile::tempdir().expect("tempdir");
    // SAFETY: this binary's one test sets the variable before anything reads
    // it, on the only thread that does.
    unsafe {
        std::env::set_var("DAT0_CACHE_DIR", dir.path());
        std::env::set_var("RUST_LOG", "info");
    }
    assert!(dat0_core::boot::init_logging().is_ok());
    tracing::info!("the log reaches a file");

    let log = dat0_core::boot::log_dir()
        .expect("log dir")
        .join("dat0.log");
    assert!(log.starts_with(dir.path()), "{}", log.display());
    let text = std::fs::read_to_string(&log).expect("the log file");
    assert!(text.contains("the log reaches a file"), "{text:?}");
    assert!(!text.contains('\u{1b}'), "no terminal colours in a file");
}

#[test]
fn a_log_grown_past_its_limit_is_set_aside_at_launch() {
    let dir = tempfile::tempdir().expect("tempdir");
    std::fs::write(dir.path().join("dat0.log"), vec![b'x'; 64]).unwrap();

    // Under the limit: appended to.
    drop(dat0_core::boot::open_log_file(dir.path(), 100).unwrap());
    assert!(!dir.path().join("dat0.log.1").exists());

    // Past it: set aside, and a new one begun.
    drop(dat0_core::boot::open_log_file(dir.path(), 10).unwrap());
    assert_eq!(
        std::fs::read(dir.path().join("dat0.log.1")).unwrap().len(),
        64
    );
    assert_eq!(
        std::fs::metadata(dir.path().join("dat0.log"))
            .unwrap()
            .len(),
        0
    );
}
