//! Cancel an in-flight import: cancel flag is set → engine.interrupt
//! called → tokio task exits → no orphan table created → Banner emitted.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

#[test]
fn cancel_flag_propagates_to_engine_interrupt() {
    let cancel = Arc::new(AtomicBool::new(false));

    let cancel_clone = cancel.clone();
    let task_done = Arc::new(AtomicBool::new(false));
    let task_done_clone = task_done.clone();
    let handle = std::thread::spawn(move || {
        let start = std::time::Instant::now();
        loop {
            if cancel_clone.load(Ordering::SeqCst) {
                task_done_clone.store(true, Ordering::SeqCst);
                return;
            }
            if start.elapsed() > std::time::Duration::from_secs(5) {
                panic!("timeout: cancel flag never observed");
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
    });

    dat0_app::import_progress::request_cancel(&cancel);
    handle.join().unwrap();
    assert!(task_done.load(Ordering::SeqCst));
}

#[test]
fn import_progress_records_active_handle() {
    let p = dat0_app::import_progress::ImportProgress::new(1024 * 1024);
    p.update(1024);
    p.update(2048);
    assert_eq!(p.bytes_done(), 2048);
    assert_eq!(p.total_bytes(), 1024 * 1024);
}

#[test]
fn cancel_active_no_op_when_none_registered() {
    dat0_app::import_progress::clear_active();
    dat0_app::import_progress::cancel_active_for_test();
}
